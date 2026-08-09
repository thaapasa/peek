//! Session-stack layer of the interactive viewer: [`SessionFrame`] (one
//! peek session — source + detected type + mode stack + per-mode
//! scroll / view caches) plus the `ViewerState` operations that grow and
//! shrink the recursive-peek stack — descend, extract, push / pop — and
//! the position capture / restore that keeps a mode's place across
//! mode switches.

use anyhow::Result;
use peek_detect::Detected;
use peek_foundation::extract::Extracted;
use peek_foundation::info::FileInfo;
use peek_foundation::viewer::modes::{Mode, ModeId, ParentNav, Position};
use peek_io::InputSource;

use super::render::RenderedView;
use super::state::ViewerState;

/// Hard cap on session-stack depth. Real listings rarely nest beyond
/// 3–4 levels; the cap exists so a hostile container that recursively
/// resolves to itself can't grow the stack without bound.
const MAX_STACK_DEPTH: usize = 16;

/// One peek session — one `(source, detected, modes)` triple plus its
/// per-mode scroll / view cache / position state. The recursive-peek
/// stack is a `Vec<SessionFrame>`; the active session is always the
/// last entry. Cross-session state (theme, prompt overlay, screen
/// buffer) lives directly on `ViewerState`.
pub(crate) struct SessionFrame {
    pub source: InputSource,
    pub detected: Detected,
    pub file_info: FileInfo,
    pub modes: Vec<Box<dyn Mode>>,
    pub active: usize,
    /// Most recent primary (non-aux) mode. Aux toggles return here.
    /// `None` when no primary modes exist (binary files where Hex is
    /// the only data view).
    pub last_primary: Option<usize>,
    pub scroll: Vec<usize>,
    pub views: Vec<Option<RenderedView>>,
    /// Last known logical position; restored when modes that track
    /// position become active again.
    pub position: Position,
    /// One-shot retry guard: when a render fails on this frame, we try
    /// re-detecting the source with `detect_ignore_name` and rebuild the
    /// frame. Set after that retry runs (success or not) so a second
    /// render failure on the rebuilt frame propagates rather than
    /// looping.
    pub retry_attempted: bool,
    /// Overrides `source.name()` in the breadcrumb when set. Used by
    /// synthetic descend frames that reuse the parent source (SQLite
    /// table view) so the crumb shows the table name, not the db file
    /// repeated.
    pub breadcrumb_label: Option<String>,
    /// Set when this frame's expensive open was held back (the source is
    /// big and the session is still [`Default`](super::Access::Default)) —
    /// a transparent decompress or a compressed-tar TOC walk. The frame
    /// lands on Info with a load prompt; pressing Enter runs the deferred
    /// work and reseeds the frame to the real content. `None` once loaded
    /// (or never deferred). See [`super::Deferred`].
    pub deferred: Option<super::Deferred>,
}

impl SessionFrame {
    pub(super) fn new(
        source: InputSource,
        detected: Detected,
        file_info: FileInfo,
        modes: Vec<Box<dyn Mode>>,
    ) -> Self {
        assert!(!modes.is_empty(), "SessionFrame needs at least one mode");
        let n = modes.len();
        let last_primary = if modes[0].is_aux() { None } else { Some(0) };
        Self {
            source,
            detected,
            file_info,
            modes,
            active: 0,
            last_primary,
            scroll: vec![0; n],
            views: (0..n).map(|_| None).collect(),
            position: Position::Unknown,
            retry_attempted: false,
            breadcrumb_label: None,
            deferred: None,
        }
    }

    /// Switch the active mode to the Info view, if present. Used when a
    /// frame opens deferred so the user lands on the codec / size summary
    /// (with the load prompt) rather than the raw-byte Hex dump.
    pub(super) fn focus_info(&mut self) {
        if let Some(idx) = self.mode_index(ModeId::Info) {
            self.active = idx;
        }
    }

    /// Replace the mode stack and reset all per-mode caches (active,
    /// last_primary, scroll, views, position) to the same shape
    /// `SessionFrame::new` would produce. Used by the retry-detection
    /// path so a future tweak to `new`'s reset rules flows here too.
    pub(super) fn reseed_from_modes(&mut self, modes: Vec<Box<dyn Mode>>) {
        assert!(!modes.is_empty(), "SessionFrame needs at least one mode");
        let n = modes.len();
        self.last_primary = if modes[0].is_aux() { None } else { Some(0) };
        self.modes = modes;
        self.active = 0;
        self.scroll = vec![0; n];
        self.views = (0..n).map(|_| None).collect();
        self.position = Position::Unknown;
    }

    pub(super) fn mode_index(&self, id: ModeId) -> Option<usize> {
        self.modes.iter().position(|m| m.id() == id)
    }
}

impl ViewerState {
    /// Display names of every frame on the stack (root first), used
    /// by the status line to render the breadcrumb segment.
    pub(crate) fn breadcrumb(&self) -> Vec<String> {
        self.frames
            .iter()
            .map(|f| {
                f.breadcrumb_label
                    .clone()
                    .unwrap_or_else(|| f.source.name().to_string())
            })
            .collect()
    }

    pub(super) fn extract_target_key(&mut self) -> Option<String> {
        let f = self.frame();
        let target = f.modes[f.active].extract_target()?;
        Some(match target {
            peek_foundation::viewer::modes::ExtractTarget::EntryPath(p) => p,
            peek_foundation::viewer::modes::ExtractTarget::FrameIndex(n) => n.to_string(),
        })
    }

    /// Declared size of the active mode's current extract selection, if
    /// the mode reports one. Drives the large-extract confirmation.
    fn selected_extract_size(&self) -> Option<u64> {
        let f = self.frame();
        f.modes[f.active].selected_extract_size()
    }

    /// Open a confirmation prompt when the selection is large and the
    /// session is still locked, returning `true` so the caller defers the
    /// extract until confirm. Returns `false` to proceed immediately
    /// (small selection, unknown size, or already unlocked).
    fn maybe_confirm_extract(&mut self, key: &str, save: bool) -> bool {
        if self.access != super::Access::Unlocked
            && let Some(sz) = self.selected_extract_size()
            && sz > super::EXTRACT_PROMPT_BYTES
        {
            self.begin_confirm_extract(key.to_string(), sz, save);
            return true;
        }
        false
    }

    /// Run extract against the active mode's selection, then open the
    /// save-to prompt. Failures flash on the status line. A large
    /// selection asks for confirmation first.
    pub(super) fn start_extract(&mut self) {
        let Some(key) = self.extract_target_key() else {
            self.flash = Some("nothing selected to extract".to_string());
            return;
        };
        if self.maybe_confirm_extract(&key, true) {
            return;
        }
        self.run_extract_save(key);
    }

    /// Extract `key` and open the save-to prompt. The post-confirmation
    /// tail of [`start_extract`], also reached straight from the confirm
    /// prompt.
    pub(super) fn run_extract_save(&mut self, key: String) {
        let opts = peek_foundation::extract::ExtractOptions {
            no_tempfile: self.no_tempfile,
            ..Default::default()
        };
        let f = self.frame();
        match crate::extract::extract(&f.source, &f.detected, &key, &opts) {
            Ok(extracted) => self.begin_extract_prompt(extracted),
            Err(e) => self.flash = Some(format!("extract failed: {e}")),
        }
    }

    /// Recursive peek: extract the active mode's selection and push it
    /// as a new session on the stack. Failures (no selection,
    /// unsupported, broken entry, stack full) flash and leave the
    /// current frame active.
    pub(super) fn descend(&mut self) -> Result<()> {
        // A deferred-decompress frame has no entry to descend into — Enter
        // means "load the inner content" instead.
        if self.frame().deferred.is_some() {
            return self.load_deferred();
        }
        if self.frames.len() >= MAX_STACK_DEPTH {
            self.flash = Some(format!("peek stack at max depth ({MAX_STACK_DEPTH})"));
            return Ok(());
        }
        let frame_idx = self.active_frame_idx();
        let active = self.frames[frame_idx].active;
        // In-frame jump (e.g. object-file symbol → its byte offset in the
        // Hex view) switches the active mode without touching the stack.
        if let Some((mode_id, pos)) = self.frames[frame_idx].modes[active].select_jump() {
            self.jump_to_position(mode_id, pos);
            return Ok(());
        }
        // Mode-provided direct frame (e.g. SQLite table → row viewer)
        // bypasses the extract pipeline entirely.
        if let Some(result) = self.frames[frame_idx].modes[active].build_descend_frame() {
            return match result {
                Ok(frame) => self.push_direct_frame(frame),
                Err(e) => {
                    self.flash = Some(format!("descend failed: {e:#}"));
                    Ok(())
                }
            };
        }
        let Some(key) = self.extract_target_key() else {
            self.flash = Some("nothing to descend into".to_string());
            return Ok(());
        };
        if self.maybe_confirm_extract(&key, false) {
            return Ok(());
        }
        self.run_descend_extract(key)
    }

    /// Go up one directory in a listing view (`Backspace`). Asks the
    /// active mode how it wants to ascend: a tree TOC moves its own
    /// selection up a level in place; the on-disk directory browser
    /// descends into its `..` row, which [`push_extracted`](Self::push_extracted)
    /// re-targets onto the current frame and seeds at the child we left.
    pub(super) fn parent_dir(&mut self) -> Result<()> {
        let nav = {
            let f = self.frame_mut();
            let active = f.active;
            f.modes[active].parent_nav()
        };
        match nav {
            ParentNav::Handled => {
                // The mode moved its selection itself — refresh its view.
                self.invalidate_active();
                Ok(())
            }
            ParentNav::Descend(key) => self.run_descend_extract(key),
            ParentNav::None => {
                self.flash = Some("already at top level".to_string());
                Ok(())
            }
        }
    }

    /// Extract `key` and push it as a new session frame. The
    /// post-confirmation tail of [`descend`](Self::descend), also reached
    /// straight from the confirm prompt.
    pub(super) fn run_descend_extract(&mut self, key: String) -> Result<()> {
        let opts = peek_foundation::extract::ExtractOptions {
            no_tempfile: self.no_tempfile,
            ..Default::default()
        };
        let extracted = {
            let f = self.frame();
            match crate::extract::extract(&f.source, &f.detected, &key, &opts) {
                Ok(e) => e,
                Err(e) => {
                    self.flash = Some(format!("descend failed: {e}"));
                    return Ok(());
                }
            }
        };
        self.push_extracted(extracted)
    }

    /// Push a mode-supplied descend frame onto the session stack
    /// without going through extract / re-detect. Used by modes that
    /// already know the source, file type, and modes for the next
    /// frame (SQLite contents → row viewer). Gathers `FileInfo` from
    /// the supplied source + detected so the new frame's InfoMode has
    /// a populated panel.
    pub(super) fn push_direct_frame(
        &mut self,
        frame: peek_foundation::viewer::modes::DescendFrame,
    ) -> Result<()> {
        let peek_foundation::viewer::modes::DescendFrame {
            source,
            detected,
            modes,
            breadcrumb_label,
        } = frame;
        let file_info = match crate::gather::gather(&source, &detected) {
            Ok(info) => info,
            Err(e) => {
                self.flash = Some(format!("descend failed: {e:#}"));
                return Ok(());
            }
        };
        let mut session = SessionFrame::new(source, detected, file_info, modes);
        session.breadcrumb_label = breadcrumb_label;
        self.frames.push(session);
        self.screen.invalidate();
        Ok(())
    }

    fn active_frame_idx(&self) -> usize {
        self.frames.len() - 1
    }

    /// Run the deferred work on the active frame, then rebuild its mode
    /// stack in place and unlock the session so later guarded ops proceed
    /// without re-asking. Uniform across the deferral kinds:
    /// `resolve_transparent` expands a deferred decompress (and is a no-op
    /// on a deferred-listing archive), then the mode builder composes the
    /// real stack — the inner content, or the full compressed-tar TOC walk.
    /// On failure the frame still reseeds and `deferred` clears, so Enter
    /// is not a dead key on a broken source.
    fn load_deferred(&mut self) -> Result<()> {
        let (source, detected) = {
            let f = self.frame();
            peek_detect::resolve_transparent(f.source.clone(), f.detected.clone())
        };
        let modes = match (self.mode_builder)(&source, &detected) {
            Ok(m) => m,
            Err(e) => {
                self.flash = Some(format!("load failed: {e}"));
                return Ok(());
            }
        };
        let file_info = crate::gather::gather(&source, &detected)?;
        let f = self.frame_mut();
        f.source = source;
        f.detected = detected;
        f.file_info = file_info;
        f.deferred = None;
        f.reseed_from_modes(modes);
        self.access = super::Access::Unlocked;
        self.screen.invalidate();
        Ok(())
    }

    fn push_extracted(&mut self, extracted: Extracted) -> Result<()> {
        // Walking up to a parent directory (`..`): the new frame should
        // open with the child we came from selected, not the top of the
        // list. The came-from name is the current directory's basename;
        // the parent listing has a row for it.
        let came_from = (extracted.suggested_name == "..")
            .then(|| {
                self.frame()
                    .source
                    .disk_path()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            })
            .flatten();
        let source = extracted.source;
        let detected = match peek_detect::detect(&source) {
            Ok(d) => d,
            Err(e) => {
                self.flash = Some(format!("descend failed: {e}"));
                return Ok(());
            }
        };
        // Apply transparent decompression so descending into an
        // extracted `.gz` / `.bz2` / `.xz` / `.zst` / `.lz4` lands
        // straight on the inner content — unless it's big and the session
        // is still Default, in which case defer (same latency guard as the
        // top-level open: a decompress or a compressed-tar TOC walk) and
        // land on Info with a load prompt.
        let deferred = super::deferred_open(&source, &detected, self.access);
        let (source, detected) = if deferred.is_some() {
            (source, detected)
        } else {
            peek_detect::resolve_transparent(source, detected)
        };
        let modes = match super::compose_or_defer(deferred, &source, || {
            (self.mode_builder)(&source, &detected)
        }) {
            Ok(m) => m,
            Err(e) => {
                self.flash = Some(format!("descend failed: {e}"));
                return Ok(());
            }
        };
        let file_info = crate::gather::gather(&source, &detected)?;
        let mut frame = SessionFrame::new(source, detected, file_info, modes);
        frame.deferred = deferred;
        if deferred.is_some() {
            frame.focus_info();
        }
        // Seed the parent listing's cursor on the directory we came from.
        if let Some(name) = came_from {
            let active = frame.active;
            frame.modes[active].select_entry(&name);
        }
        // Dir → Dir descent re-targets the current frame instead of
        // pushing, so navigating between sibling subdirectories doesn't
        // accumulate a stack the user has to back out of. Esc on the
        // resulting frame still exits peek (depth-1 Back semantics).
        let collapse = matches!(frame.detected.file_type, peek_detect::FileType::Directory)
            && matches!(
                self.frame().detected.file_type,
                peek_detect::FileType::Directory
            );
        if collapse {
            *self.frames.last_mut().expect("non-empty stack") = frame;
        } else {
            self.frames.push(frame);
        }
        self.screen.invalidate();
        Ok(())
    }

    pub(super) fn pop_frame(&mut self) {
        if self.frames.len() <= 1 {
            return;
        }
        self.frames.pop();
        self.screen.invalidate();
    }
}

pub(super) fn capture_position(f: &mut SessionFrame) {
    let mode = &f.modes[f.active];
    if !mode.tracks_position() {
        return;
    }
    let pos = if mode.owns_scroll() {
        mode.position()
    } else {
        Position::Line(f.scroll[f.active])
    };
    if !matches!(pos, Position::Unknown) {
        f.position = pos;
    }
}

pub(super) fn restore_position(f: &mut SessionFrame) {
    let pos = f.position;
    let active = f.active;
    let source = f.source.clone();
    let mode = &mut f.modes[active];
    if !mode.tracks_position() {
        return;
    }
    if mode.owns_scroll() {
        mode.set_position(pos, &source);
        return;
    }
    let line = match pos {
        Position::Line(l) => Some(l),
        Position::Byte(b) => source.byte_to_line(b),
        Position::Unknown => None,
    };
    if let Some(l) = line {
        f.scroll[active] = l;
    }
}
