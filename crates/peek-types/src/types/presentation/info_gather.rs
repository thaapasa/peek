//! Gather presentation extras for the Info section. PPTX / ODP parse
//! the deck (slide / word / image counts + metadata); Keynote surfaces
//! only the cheap metadata (no slide-text parse).

use anyhow::Result;

use crate::info::Extras;
use crate::input::InputSource;

use super::{Deck, PresentationFormat, PresentationStats};

pub fn gather_extras(source: &InputSource, fmt: PresentationFormat) -> Extras {
    match fmt {
        PresentationFormat::Pptx | PresentationFormat::Pptm | PresentationFormat::Ppsx => {
            from_deck(fmt, super::pptx::package::open(source))
        }
        PresentationFormat::Odp => from_deck(fmt, super::odp::package::open(source)),
        PresentationFormat::Key => match super::keynote::package::open(source) {
            Ok(kn) => Box::new(PresentationStats {
                format: fmt,
                metadata: kn.metadata,
                slide_count: 0,
                word_count: 0,
                image_count: 0,
            }),
            Err(_) => Box::new(PresentationStats::empty(fmt)),
        },
    }
}

fn from_deck(fmt: PresentationFormat, parsed: Result<Deck>) -> Extras {
    match parsed {
        Ok(deck) => Box::new(PresentationStats {
            format: fmt,
            slide_count: deck.slide_count(),
            word_count: deck.word_count(),
            image_count: deck.image_count(),
            metadata: deck.metadata,
        }),
        Err(_) => Box::new(PresentationStats::empty(fmt)),
    }
}
