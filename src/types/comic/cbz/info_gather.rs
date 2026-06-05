//! Gather CBZ-specific extras for the Info section.

use crate::info::Extras;
use crate::input::InputSource;
use crate::input::detect::ComicFormat;
use crate::types::comic::ComicStats;

use super::package;

pub fn gather_extras(source: &InputSource, format: ComicFormat) -> Extras {
    match package::list_pages(source) {
        Ok(pages) => {
            let total: u64 = pages.iter().map(|p| p.size).sum();
            Box::new(ComicStats {
                format,
                page_count: pages.len(),
                total_image_bytes: total,
            })
        }
        Err(_) => Box::new(ComicStats {
            format,
            page_count: 0,
            total_image_bytes: 0,
        }),
    }
}
