//! Generate `test-audio/sample-tagged.mp3` — a tiny MP3 with embedded
//! cover art + unsynced lyrics + the standard ID3v2 tag fields. Used
//! by the audio listing + extract tests.
//!
//! Usage: `cargo run --example gen_audio_fixture`
//!
//! Output layout:
//!   ID3v2.4 tag (TIT2 / TPE1 / TALB / APIC front + back / USLT)
//!   one MPEG-1 Layer 3 silent frame (417 bytes, all-zero main data)
//!
//! The MP3 frame doesn't decode meaningfully — symphonia identifies
//! the file as MP3 from the sync word + header bits, which is all the
//! info / listing / extract paths need.

use std::io::Write;

use id3::frame::{Lyrics, Picture, PictureType};
use id3::{Tag, TagLike, Version};

fn main() {
    let mp3_frame = silent_mp3_frame();
    let front = png_one_by_one(0xff, 0x00, 0x00); // red
    let back = png_one_by_one(0x00, 0x00, 0xff); // blue

    let mut tag = Tag::new();
    tag.set_title("Sample Track");
    tag.set_artist("Test Artist");
    tag.set_album("Test Album");
    tag.set_album_artist("Test Artist");
    tag.set_year(2026);
    tag.set_genre("Test");
    tag.set_track(1);

    tag.add_frame(Picture {
        mime_type: "image/png".to_string(),
        picture_type: PictureType::CoverFront,
        description: "front".to_string(),
        data: front,
    });
    tag.add_frame(Picture {
        mime_type: "image/png".to_string(),
        picture_type: PictureType::CoverBack,
        description: "back".to_string(),
        data: back,
    });
    tag.add_frame(Lyrics {
        lang: "eng".to_string(),
        description: "Main".to_string(),
        text: "la la la\nfa la la\nthe end".to_string(),
    });

    let out_path = std::path::Path::new("test-audio").join("sample-tagged.mp3");
    let mut out = std::fs::File::create(&out_path)
        .expect("failed to create fixture file; run from repo root");
    tag.write_to(&mut out, Version::Id3v24)
        .expect("ID3 write failed");
    out.write_all(&mp3_frame).expect("frame write failed");
    drop(out);
    let size = std::fs::metadata(&out_path).expect("stat").len();
    eprintln!("wrote {} ({} bytes)", out_path.display(), size);
}

/// One MPEG-1 Layer 3 frame, mono, 128 kbps, 44.1 kHz, no CRC, with
/// all-zero side info + main data. Symphonia identifies the codec from
/// the header alone, so this 417-byte frame is enough to exercise the
/// probe / listing / extract code paths.
fn silent_mp3_frame() -> Vec<u8> {
    let mut frame = vec![0u8; 417];
    // Sync word + flags.
    // 0xFF 0xFB: 11 sync bits + MPEG-1 (11) + Layer 3 (01) + no CRC (1)
    // 0x90: bitrate index 1001 (128 kbps) + sample rate index 00 (44.1 kHz) + no pad + no priv
    // 0xC0: channel mode 11 (mono) + rest zero
    frame[0] = 0xff;
    frame[1] = 0xfb;
    frame[2] = 0x90;
    frame[3] = 0xc0;
    frame
}

/// Hand-rolled 1×1 RGB PNG with the given pixel colour. ~70 bytes —
/// keeps the fixture tiny while still carrying valid PNG magic so the
/// image pipeline detects it correctly on recursive peek.
fn png_one_by_one(r: u8, g: u8, b: u8) -> Vec<u8> {
    use std::io::Write;
    let mut bytes: Vec<u8> = Vec::with_capacity(80);
    // PNG signature.
    bytes.extend_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    // IHDR chunk: width=1, height=1, bit depth=8, colour type=2 (RGB).
    write_chunk(
        &mut bytes,
        b"IHDR",
        &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0],
    );
    // IDAT: zlib-wrapped deflate stream of one scanline `[0 r g b]`.
    let scanline = [0, r, g, b];
    let mut z: Vec<u8> = Vec::new();
    // Stored deflate block: BFINAL=1 BTYPE=00 (uncompressed).
    // zlib header (CMF=0x78, FLG checked).
    z.push(0x78);
    z.push(0x01);
    z.push(0x01); // BFINAL=1, BTYPE=00
    let len: u16 = scanline.len() as u16;
    z.write_all(&len.to_le_bytes()).unwrap();
    z.write_all(&(!len).to_le_bytes()).unwrap();
    z.write_all(&scanline).unwrap();
    let adler = adler32(&scanline);
    z.write_all(&adler.to_be_bytes()).unwrap();
    write_chunk(&mut bytes, b"IDAT", &z);
    // IEND chunk.
    write_chunk(&mut bytes, b"IEND", &[]);
    bytes
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input: Vec<u8> = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb88320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}
