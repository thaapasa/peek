//! AVIF decode, plugged into the `image` crate as a decoding hook.
//!
//! `image`'s own AVIF decoder needs the dav1d C library; this one uses
//! `avif-decode` (pure-Rust rav1d). Registered once at startup via
//! [`register`], after which every `ImageReader::with_guessed_format()`
//! path (viewer, `--info`, epub / comic pages) decodes AVIF unchanged.
//!
//! The decoder parses the container up front for dimensions and layout,
//! so `--info` and `image`'s allocation limit check run without an AV1
//! decode. The pixel decode happens in `read_image`.
//!
//! Unlike `image`'s streaming decoders, AV1 needs the whole file in
//! memory, so even the File arm buffers it: gated by the render cap.

use std::io::{Read, Seek, SeekFrom};
use std::sync::Once;

use avif_decode::Image;
use image::error::{DecodingError, ImageFormatHint};
use image::hooks::{self, GenericReader};
use image::{ColorType, ImageDecoder, ImageError, ImageFormat, ImageResult};

const EXT: &str = "avif";

/// Register the AVIF decoding + format-detection hooks. Idempotent.
pub fn register() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        hooks::register_decoding_hook(EXT.into(), Box::new(|r| Ok(Box::new(AvifDecoder::new(r)?))));
        // `ftyp` box with the AVIF brand; the 4-byte box size varies.
        const MASK: &[u8] = &[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        hooks::register_format_detection_hook(EXT.into(), b"\0\0\0\0ftypavif", Some(MASK));
        hooks::register_format_detection_hook(EXT.into(), b"\0\0\0\0ftypavis", Some(MASK));
    });
}

struct AvifDecoder {
    data: Vec<u8>,
    width: u32,
    height: u32,
    color_type: ColorType,
}

impl AvifDecoder {
    fn new(mut reader: GenericReader<'_>) -> ImageResult<Self> {
        let len = reader.seek(SeekFrom::End(0))? - reader.seek(SeekFrom::Start(0))?;
        if let Some(msg) = crate::viewer::modes::render_cap_exceeded(len, "AVIF image") {
            return Err(decoding_error(msg));
        }
        let mut data = Vec::new();
        reader.take(len).read_to_end(&mut data)?;
        let avif = avif_parse::read_avif(&mut data.as_slice()).map_err(decoding_error)?;
        let meta = avif.primary_item_metadata().map_err(decoding_error)?;
        // Mirrors avif-decode's output choice: alpha forces RGBA (grey
        // included), anything above 8 bits decodes to 16-bit channels.
        let wide = meta.bit_depth > 8;
        let color_type = match (avif.alpha_item.is_some(), meta.monochrome, wide) {
            (true, _, false) => ColorType::Rgba8,
            (true, _, true) => ColorType::Rgba16,
            (false, true, false) => ColorType::L8,
            (false, true, true) => ColorType::L16,
            (false, false, false) => ColorType::Rgb8,
            (false, false, true) => ColorType::Rgb16,
        };
        Ok(Self {
            data,
            width: meta.max_frame_width.get(),
            height: meta.max_frame_height.get(),
            color_type,
        })
    }
}

impl ImageDecoder for AvifDecoder {
    fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn color_type(&self) -> ColorType {
        self.color_type
    }

    fn read_image(self, buf: &mut [u8]) -> ImageResult<()> {
        let decoder = avif_decode::Decoder::from_avif(&self.data).map_err(decoding_error)?;
        let img = decoder.to_image().map_err(decoding_error)?;
        // 16-bit samples go out as native-endian bytes, which is what
        // `image` expects — exactly the in-memory layout of `u16` pixels.
        let (w, h, src): (usize, usize, &[u8]) = match &img {
            Image::Rgb8(i) => (i.width(), i.height(), bytemuck::cast_slice(i.buf())),
            Image::Rgb16(i) => (i.width(), i.height(), bytemuck::cast_slice(i.buf())),
            Image::Rgba8(i) => (i.width(), i.height(), bytemuck::cast_slice(i.buf())),
            Image::Rgba16(i) => (i.width(), i.height(), bytemuck::cast_slice(i.buf())),
            Image::Gray8(i) => (i.width(), i.height(), bytemuck::cast_slice(i.buf())),
            Image::Gray16(i) => (i.width(), i.height(), bytemuck::cast_slice(i.buf())),
        };
        // A frame-size override can shrink the frame below the sequence
        // header's maximum; the caller sized `buf` from the latter.
        if (w as u32, h as u32) != (self.width, self.height) || src.len() != buf.len() {
            return Err(decoding_error(
                "decoded frame differs from container metadata",
            ));
        }
        buf.copy_from_slice(src);
        Ok(())
    }

    fn read_image_boxed(self: Box<Self>, buf: &mut [u8]) -> ImageResult<()> {
        (*self).read_image(buf)
    }
}

fn decoding_error(err: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> ImageError {
    ImageError::Decoding(DecodingError::new(
        ImageFormatHint::Exact(ImageFormat::Avif),
        err,
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use image::{DynamicImage, ImageReader};

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-images");
        p.push(name);
        p
    }

    /// Decode through the same path the viewer uses: magic-byte guess,
    /// then the registered hook.
    fn decode(name: &str) -> DynamicImage {
        register();
        ImageReader::open(fixture(name))
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap()
    }

    /// Mean absolute per-channel difference, in 8-bit units.
    fn mean_diff(a: &DynamicImage, b: &DynamicImage) -> f64 {
        let (a, b) = (a.to_rgba8(), b.to_rgba8());
        assert_eq!(a.dimensions(), b.dimensions());
        let sum: u64 = a
            .as_raw()
            .iter()
            .zip(b.as_raw())
            .map(|(&x, &y)| u64::from(x.abs_diff(y)))
            .sum();
        sum as f64 / a.as_raw().len() as f64
    }

    #[test]
    fn decodes_8bit_420_close_to_source() {
        let img = decode("heron.avif");
        assert_eq!(img.color(), ColorType::Rgb8);
        let diff = mean_diff(&img, &image::open(fixture("heron.jpg")).unwrap());
        assert!(diff < 4.0, "mean diff {diff}");
    }

    #[test]
    fn decodes_alpha_close_to_source() {
        let img = decode("clover-alpha.avif");
        assert_eq!(img.color(), ColorType::Rgba8);
        let diff = mean_diff(&img, &image::open(fixture("clover.png")).unwrap());
        assert!(diff < 4.0, "mean diff {diff}");
    }

    /// 10-bit samples must scale to the full 16-bit range — left at
    /// 0..1023 the image would read as near-black.
    #[test]
    fn decodes_10bit_close_to_source() {
        let img = decode("clover-10bit-444.avif");
        assert_eq!(img.color(), ColorType::Rgba16);
        let diff = mean_diff(&img, &image::open(fixture("clover.png")).unwrap());
        assert!(diff < 4.0, "mean diff {diff}");
    }

    /// Dimensions come from the container, before any AV1 decode.
    #[test]
    fn reports_dimensions_from_memory_without_decoding() {
        register();
        let bytes = std::fs::read(fixture("heron.avif")).unwrap();
        let decoder = ImageReader::new(std::io::Cursor::new(bytes))
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(decoder.dimensions(), (1500, 1200));
    }

    #[test]
    fn truncated_file_errors_cleanly() {
        register();
        let mut bytes = std::fs::read(fixture("heron.avif")).unwrap();
        bytes.truncate(bytes.len() / 2);
        assert!(image::load_from_memory(&bytes).is_err());
    }
}
