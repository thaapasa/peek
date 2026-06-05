pub mod help;

// `PrintOutput` moved to the `peek-foundation` crate (the foundation's
// pipe writer). Re-exported so the bin's `crate::output::PrintOutput`
// paths (main, extract) stay unchanged.
pub use peek_foundation::output::PrintOutput;
