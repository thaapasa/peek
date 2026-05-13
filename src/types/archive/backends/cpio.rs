//! cpio TOC listing and entry extract. Hand-rolled parser for the
//! two ASCII header formats seen in practice:
//!   - newc (SVR4) magic `070701` and newc-with-CRC `070702`:
//!     110-byte header, 8-byte hex fields, 4-byte alignment on
//!     header+name and on body.
//!   - ODC (POSIX portable) magic `070707`: 76-byte header, octal
//!     fields, no padding.
//!
//! Old binary cpio (16-bit little-endian header, raw magic
//! `0xc7 0x71`) is intentionally skipped — extinct in real-world
//! archives.
//!
//! TOC walk reads only the header chain; entry bodies stream into
//! `io::sink()` so listings stay streaming-friendly for large
//! initramfs / RPM payloads. Extract pulls a single matching entry
//! by path with the same parser.
//!
//! All records in an archive share the same header format. The
//! variant is locked in from the first record's magic and reused
//! for every subsequent entry — mixed-variant archives are not a
//! real-world shape and would error on the second record.
//!
//! cpio mtimes are 32-bit Unix seconds (signed in spec, treated as
//! u64 here — negative timestamps don't appear in practice).

use std::io::{self, Read};

use anyhow::{Context, Result, anyhow, bail};

use crate::types::archive::reader::ReadSeek;
use crate::viewer::listing::{EntryMtime, FlatEntry, time_from_epoch_secs};

const NEWC_HEADER: usize = 110;
const ODC_HEADER: usize = 76;
const TRAILER: &str = "TRAILER!!!";
const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;
/// Sanity cap on `namesize` — real paths fit comfortably under this,
/// and bogus headers would otherwise allocate gigabytes.
const MAX_NAMESIZE: usize = 4096;

pub(crate) fn list_plain(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(reader)
}

pub(crate) fn list_gz(reader: Box<dyn ReadSeek>) -> Result<Vec<FlatEntry>> {
    list_from_read(flate2::read::GzDecoder::new(reader))
}

fn list_from_read<R: Read>(reader: R) -> Result<Vec<FlatEntry>> {
    let mut cpio = CpioReader::new(reader);
    let mut out = Vec::new();
    while let Some(hdr) = cpio.next_header()? {
        out.push(FlatEntry {
            path: hdr.path,
            size: hdr.size,
            mtime: hdr
                .mtime
                .and_then(|t| time_from_epoch_secs(t as u64))
                .map(EntryMtime::Utc),
            mode: Some(hdr.mode & 0o7777),
            is_dir: hdr.is_dir,
        });
    }
    Ok(out)
}

/// Stream-search the cpio archive for `target` (already path-sanitised
/// by the caller). Returns the body bytes for the first matching entry
/// or `Ok(None)` if the archive ends without a hit. `max_bytes` caps
/// extraction so a runaway entry can't force a multi-GB allocation.
///
/// `target` is matched against the entry path with a leading `./`
/// trimmed off — newc cpio commonly stores entries as `./foo/bar`.
pub(crate) fn find_entry<R: Read>(
    reader: R,
    target: &str,
    max_bytes: u64,
) -> Result<Option<Vec<u8>>> {
    let mut cpio = CpioReader::new(reader);
    while let Some(hdr) = cpio.next_header()? {
        let stored = hdr.path.trim_start_matches("./").trim_start_matches('/');
        if stored == target {
            if hdr.size > max_bytes {
                bail!(
                    "cpio entry {target:?} is {} bytes; cap is {max_bytes} bytes",
                    hdr.size
                );
            }
            return Ok(Some(cpio.read_body()?));
        }
    }
    Ok(None)
}

#[derive(Clone, Copy)]
enum Variant {
    Newc,
    Odc,
}

struct EntryHeader {
    path: String,
    size: u64,
    mtime: Option<u32>,
    mode: u32,
    is_dir: bool,
}

/// State machine over the cpio header chain. Locks the header variant
/// from the first record's magic and tracks pending body + padding so
/// the caller can either skip (default in `next_header`) or pull
/// bytes via `read_body` before advancing.
struct CpioReader<R: Read> {
    inner: R,
    variant: Option<Variant>,
    body_remaining: u64,
    body_pad: u64,
}

impl<R: Read> CpioReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            variant: None,
            body_remaining: 0,
            body_pad: 0,
        }
    }

    /// Consume any unread body + post-body padding for the previous
    /// entry, then parse the next header. Returns `Ok(None)` on the
    /// `TRAILER!!!` sentinel or clean EOF.
    fn next_header(&mut self) -> Result<Option<EntryHeader>> {
        self.drain_pending()?;

        let mut magic = [0u8; 6];
        match self.inner.read_exact(&mut magic) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }
        let v = match &magic {
            b"070701" | b"070702" => Variant::Newc,
            b"070707" => Variant::Odc,
            other => bail!(
                "invalid cpio magic: {:?}",
                std::str::from_utf8(other).unwrap_or("<non-ASCII>")
            ),
        };
        if let Some(prev) = self.variant
            && !matches!(
                (prev, v),
                (Variant::Newc, Variant::Newc) | (Variant::Odc, Variant::Odc)
            )
        {
            bail!("cpio mixed-variant archive (newc / ODC) not supported");
        }
        self.variant = Some(v);

        let hdr = match v {
            Variant::Newc => self.read_newc_header()?,
            Variant::Odc => self.read_odc_header()?,
        };
        if hdr.path == TRAILER {
            return Ok(None);
        }
        Ok(Some(hdr))
    }

    /// Read the current entry's body. Must be called between
    /// `next_header` calls; the body is consumed and `body_remaining`
    /// drops to zero. Post-body padding is drained on the next
    /// `next_header` call.
    fn read_body(&mut self) -> Result<Vec<u8>> {
        let n = self.body_remaining as usize;
        let mut buf = vec![0u8; n];
        self.inner
            .read_exact(&mut buf)
            .context("short cpio entry body")?;
        self.body_remaining = 0;
        Ok(buf)
    }

    fn drain_pending(&mut self) -> Result<()> {
        let total = self.body_remaining + self.body_pad;
        if total == 0 {
            return Ok(());
        }
        let n = io::copy(&mut (&mut self.inner).take(total), &mut io::sink())?;
        if n != total {
            bail!("truncated cpio archive (expected {total} more bytes)");
        }
        self.body_remaining = 0;
        self.body_pad = 0;
        Ok(())
    }

    fn read_newc_header(&mut self) -> Result<EntryHeader> {
        // 6 bytes of magic already consumed; read remaining 104.
        let mut buf = [0u8; NEWC_HEADER - 6];
        self.inner
            .read_exact(&mut buf)
            .context("short newc cpio header")?;
        // Field offsets within `buf` (each field is 8 ASCII hex chars):
        //   c_ino       0..  8
        //   c_mode      8.. 16
        //   c_uid      16.. 24
        //   c_gid      24.. 32
        //   c_nlink    32.. 40
        //   c_mtime    40.. 48
        //   c_filesize 48.. 56
        //   c_devmajor 56.. 64
        //   c_devminor 64.. 72
        //   c_rdevmajor 72.. 80
        //   c_rdevminor 80.. 88
        //   c_namesize 88.. 96
        //   c_check    96..104
        let mode = parse_hex(&buf[8..16])? as u32;
        let mtime = parse_hex(&buf[40..48])? as u32;
        let size = parse_hex(&buf[48..56])?;
        let namesize = parse_hex(&buf[88..96])? as usize;
        if namesize == 0 || namesize > MAX_NAMESIZE {
            bail!("invalid cpio namesize: {namesize}");
        }
        let mut name_buf = vec![0u8; namesize];
        self.inner
            .read_exact(&mut name_buf)
            .context("short newc cpio name")?;
        if name_buf.last() == Some(&0) {
            name_buf.pop();
        }
        let path = String::from_utf8_lossy(&name_buf).into_owned();
        // Header + name padded to a 4-byte boundary.
        let header_and_name = NEWC_HEADER + namesize;
        let header_pad = (4 - header_and_name % 4) % 4;
        if header_pad > 0 {
            let mut pad = [0u8; 3];
            self.inner
                .read_exact(&mut pad[..header_pad])
                .context("short newc cpio name padding")?;
        }
        let body_pad = (4 - (size as usize) % 4) % 4;
        let is_dir = (mode & S_IFMT) == S_IFDIR || path.ends_with('/');
        self.body_remaining = size;
        self.body_pad = body_pad as u64;
        Ok(EntryHeader {
            path,
            size,
            mtime: Some(mtime),
            mode,
            is_dir,
        })
    }

    fn read_odc_header(&mut self) -> Result<EntryHeader> {
        // 6 bytes of magic already consumed; read remaining 70.
        let mut buf = [0u8; ODC_HEADER - 6];
        self.inner
            .read_exact(&mut buf)
            .context("short ODC cpio header")?;
        // Field offsets within `buf` (octal ASCII):
        //   c_dev       0..  6  (6 chars)
        //   c_ino       6.. 12
        //   c_mode     12.. 18
        //   c_uid      18.. 24
        //   c_gid      24.. 30
        //   c_nlink    30.. 36
        //   c_rdev     36.. 42
        //   c_mtime    42.. 53  (11 chars)
        //   c_namesize 53.. 59
        //   c_filesize 59.. 70  (11 chars)
        let mode = parse_oct(&buf[12..18])? as u32;
        let mtime = parse_oct(&buf[42..53])? as u32;
        let namesize = parse_oct(&buf[53..59])? as usize;
        let size = parse_oct(&buf[59..70])?;
        if namesize == 0 || namesize > MAX_NAMESIZE {
            bail!("invalid cpio namesize: {namesize}");
        }
        let mut name_buf = vec![0u8; namesize];
        self.inner
            .read_exact(&mut name_buf)
            .context("short ODC cpio name")?;
        if name_buf.last() == Some(&0) {
            name_buf.pop();
        }
        let path = String::from_utf8_lossy(&name_buf).into_owned();
        let is_dir = (mode & S_IFMT) == S_IFDIR || path.ends_with('/');
        self.body_remaining = size;
        self.body_pad = 0;
        Ok(EntryHeader {
            path,
            size,
            mtime: Some(mtime),
            mode,
            is_dir,
        })
    }
}

fn parse_hex(s: &[u8]) -> Result<u64> {
    let s = std::str::from_utf8(s).map_err(|_| anyhow!("non-ASCII cpio hex field"))?;
    u64::from_str_radix(s, 16).map_err(|_| anyhow!("invalid cpio hex field: {s:?}"))
}

fn parse_oct(s: &[u8]) -> Result<u64> {
    let s = std::str::from_utf8(s).map_err(|_| anyhow!("non-ASCII cpio octal field"))?;
    u64::from_str_radix(s, 8).map_err(|_| anyhow!("invalid cpio octal field: {s:?}"))
}
