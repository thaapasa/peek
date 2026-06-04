//! Fat-aware object loading. `object::File::parse` rejects universal
//! (fat) Mach-O containers outright; this picks one architecture slice
//! — the host arch when the container carries it, else the first — and
//! parses that, reporting the full arch list so the Info view can
//! surface every slice.

use anyhow::{Result, anyhow, bail};
use object::read::macho::{FatArch, MachOFatFile32, MachOFatFile64};

/// A parsed object plus, for universal containers, the fat-slice summary.
pub struct Loaded<'data> {
    pub file: object::File<'data>,
    /// The exact bytes `file` was parsed from: the whole input for a
    /// plain object, or the selected slice for a universal Mach-O.
    /// Lets format-specific walks (linked libraries) re-parse the same
    /// view the rest of the Info reflects.
    pub data: &'data [u8],
    /// `Some` when the input was a fat / universal Mach-O.
    pub fat: Option<FatSummary>,
}

/// Architecture inventory of a universal (fat) Mach-O container.
pub struct FatSummary {
    /// Architecture of every slice, in container order.
    pub architectures: Vec<object::Architecture>,
    /// Index into `architectures` of the slice actually parsed.
    pub selected: usize,
}

/// Parse `data` as an object file, transparently selecting one slice of
/// a universal Mach-O container.
pub fn load(data: &[u8]) -> Result<Loaded<'_>> {
    match object::FileKind::parse(data) {
        Ok(object::FileKind::MachOFat32) => {
            let fat = MachOFatFile32::parse(data).map_err(|e| anyhow!("bad fat header: {e}"))?;
            load_fat(data, fat.arches())
        }
        Ok(object::FileKind::MachOFat64) => {
            let fat = MachOFatFile64::parse(data).map_err(|e| anyhow!("bad fat header: {e}"))?;
            load_fat(data, fat.arches())
        }
        _ => Ok(Loaded {
            file: object::File::parse(data)
                .map_err(|e| anyhow!("not a recognised object file: {e}"))?,
            data,
            fat: None,
        }),
    }
}

/// Parse one slice of a universal binary. Generic over the 32- and
/// 64-bit fat-arch records.
fn load_fat<'data, A: FatArch>(data: &'data [u8], arches: &[A]) -> Result<Loaded<'data>> {
    if arches.is_empty() {
        bail!("universal binary contains no architecture slices");
    }
    let architectures: Vec<object::Architecture> =
        arches.iter().map(|a| a.architecture()).collect();
    // Prefer the slice matching the host architecture; fall back to the
    // first slice when the container doesn't carry it.
    let host = host_architecture();
    let selected = arches
        .iter()
        .position(|a| Some(a.architecture()) == host)
        .unwrap_or(0);
    let slice = arches[selected]
        .data(data)
        .map_err(|e| anyhow!("universal slice unreadable: {e}"))?;
    let file =
        object::File::parse(slice).map_err(|e| anyhow!("universal slice not parseable: {e}"))?;
    Ok(Loaded {
        file,
        data: slice,
        fat: Some(FatSummary {
            architectures,
            selected,
        }),
    })
}

/// Architecture peek itself was built for — the preferred slice of a
/// universal binary.
fn host_architecture() -> Option<object::Architecture> {
    use object::Architecture;
    if cfg!(target_arch = "aarch64") {
        Some(Architecture::Aarch64)
    } else if cfg!(target_arch = "x86_64") {
        Some(Architecture::X86_64)
    } else {
        None
    }
}
