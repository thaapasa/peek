//! EXIF metadata extraction for raster images. Reads the in-memory head
//! buffer; callers in `image` already loaded that.

use std::io::Cursor;

pub(super) fn exif_fields_from_bytes(data: &[u8]) -> Vec<(String, String)> {
    let mut cursor = Cursor::new(data);
    exif_fields(&mut cursor)
}

fn exif_fields<R: std::io::BufRead + std::io::Seek>(reader: &mut R) -> Vec<(String, String)> {
    let exif_reader = exif::Reader::new();
    let exif = match exif_reader.read_from_container(reader) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    // Fields to extract, in display order.
    const FIELDS: &[(exif::Tag, &str)] = &[
        (exif::Tag::Make, "Camera Make"),
        (exif::Tag::Model, "Camera Model"),
        (exif::Tag::LensModel, "Lens"),
        (exif::Tag::Orientation, "Orientation"),
        (exif::Tag::XResolution, "X Resolution"),
        (exif::Tag::YResolution, "Y Resolution"),
        (exif::Tag::ResolutionUnit, "Resolution Unit"),
        (exif::Tag::ExposureTime, "Exposure"),
        (exif::Tag::FNumber, "Aperture"),
        (exif::Tag::PhotographicSensitivity, "ISO"),
        (exif::Tag::FocalLength, "Focal Length"),
        (exif::Tag::FocalLengthIn35mmFilm, "Focal Length (35mm)"),
        (exif::Tag::ExposureBiasValue, "Exposure Bias"),
        (exif::Tag::MeteringMode, "Metering"),
        (exif::Tag::Flash, "Flash"),
        (exif::Tag::WhiteBalance, "White Balance"),
        (exif::Tag::DateTimeOriginal, "Date Taken"),
        (exif::Tag::Software, "Software"),
        (exif::Tag::ImageDescription, "Description"),
        (exif::Tag::Artist, "Artist"),
        (exif::Tag::Copyright, "Copyright"),
        (exif::Tag::GPSLatitude, "GPS Latitude"),
        (exif::Tag::GPSLongitude, "GPS Longitude"),
        (exif::Tag::GPSAltitude, "GPS Altitude"),
    ];

    let mut result = Vec::new();
    for &(tag, label) in FIELDS {
        if let Some(field) = exif.get_field(tag, exif::In::PRIMARY) {
            let value = field.display_value().with_unit(&exif).to_string();
            let value = value.trim().to_string();
            if !value.is_empty() {
                result.push((label.to_string(), value));
            }
        }
    }
    result
}
