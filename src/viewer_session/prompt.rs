//! Modal-prompt plumbing for the interactive viewer: the one prompt
//! slot on `ViewerState`, the [`PromptKind`] work tag that says what a
//! confirmed value means (extract save-path vs search query), and the
//! key handling that routes raw input to the open prompt.

use anyhow::Result;
use crossterm::event::KeyEvent;
use peek_foundation::extract::Extracted;
use peek_foundation::viewer::search::{SearchQuery, SearchTarget};
use peek_foundation::viewer::ui::prompt::{Prompt, PromptOutcome};

use super::state::ViewerState;

/// What the modal [`Prompt`] is collecting input for — the action to run
/// when the user confirms. Lets one prompt slot serve both the
/// extract-save flow and text search.
pub(super) enum PromptKind {
    /// Save the extracted item to the typed path.
    Extract(Extracted),
    /// Hand the typed query to the active mode's `set_search`.
    Search,
    /// Confirm a large extract before spooling it. On confirm the session
    /// unlocks and the extract runs — pushing a frame (`save = false`,
    /// from descend) or opening the save prompt (`save = true`, from `x`).
    ConfirmExtract { key: String, save: bool },
}

/// Condense a regex compile error into a one-line status message. The
/// `regex` crate renders a multi-line parse error (the pattern, a caret,
/// then `error: <reason>`); the status bar has room only for the reason.
fn regex_reason(e: &impl std::fmt::Display) -> String {
    e.to_string()
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("error:"))
        .map(|r| r.trim().to_string())
        .unwrap_or_else(|| "invalid pattern".to_string())
}

/// Compact byte-size for prompt copy: GiB once past a gigabyte, MiB below.
fn human_bytes(n: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if n >= GIB {
        format!("{:.1} GiB", n as f64 / GIB as f64)
    } else {
        format!("{} MiB", n / MIB)
    }
}

impl ViewerState {
    pub(crate) fn prompt_active(&self) -> bool {
        self.prompt.is_some()
    }

    pub(crate) fn active_prompt(&self) -> Option<&Prompt> {
        self.prompt.as_ref().map(|(p, _)| p)
    }

    pub(crate) fn take_flash(&mut self) -> Option<String> {
        self.flash.take()
    }

    /// Open the save-to prompt; Enter writes `extracted` to the typed
    /// path, Esc drops it without writing.
    pub(crate) fn begin_extract_prompt(&mut self, extracted: Extracted) {
        let prefill = extracted.suggested_name.clone();
        self.prompt = Some((
            Prompt::new("Save to", prefill),
            PromptKind::Extract(extracted),
        ));
    }

    /// Open the text-search prompt; Enter hands the query to the active
    /// mode's `set_search`, Esc closes without changing the search.
    pub(super) fn begin_search_prompt(&mut self) {
        self.prompt = Some((Prompt::search(self.search_regex), PromptKind::Search));
    }

    /// Open the large-extract confirmation; `y` / Enter unlocks the
    /// session and runs the extract, `n` / Esc aborts.
    pub(super) fn begin_confirm_extract(&mut self, key: String, size: u64, save: bool) {
        let verb = if save { "Extract" } else { "Open" };
        let title = format!("{verb} {} entry?", human_bytes(size));
        self.prompt = Some((
            Prompt::confirm(title),
            PromptKind::ConfirmExtract { key, save },
        ));
    }

    pub(crate) fn handle_prompt_key(&mut self, key: KeyEvent) -> Result<bool> {
        let Some((prompt, _)) = self.prompt.as_mut() else {
            return Ok(false);
        };
        let outcome = prompt.handle_key(key);
        match outcome {
            PromptOutcome::Continue => Ok(true),
            PromptOutcome::Cancelled => {
                let (prompt, kind) = self.prompt.take().expect("prompt present");
                match kind {
                    PromptKind::Extract(_) | PromptKind::ConfirmExtract { .. } => {
                        self.flash = Some("extract cancelled".to_string());
                    }
                    // Remember the toggle even on a cancelled search, so a
                    // mistoggle-then-Esc still sticks for the next `/`.
                    PromptKind::Search => self.search_regex = prompt.is_regex(),
                }
                Ok(true)
            }
            PromptOutcome::Confirmed(value) => {
                let (prompt, kind) = self.prompt.take().expect("prompt present");
                match kind {
                    PromptKind::Extract(extracted) => {
                        let dest = if value.is_empty() {
                            crate::extract::write::Output::resolve(None, &extracted.suggested_name)
                        } else if value == "-" {
                            crate::extract::write::Output::Stdout
                        } else {
                            crate::extract::write::Output::Path(value.into())
                        };
                        match crate::extract::write::write_extracted(&extracted, dest) {
                            Ok(path) => {
                                self.flash = Some(format!("wrote {}", path.display()));
                            }
                            Err(e) => {
                                self.flash = Some(format!("extract failed: {e}"));
                            }
                        }
                    }
                    PromptKind::ConfirmExtract { key, save } => {
                        // User accepted the cost — unlock the session so
                        // later large ops don't re-ask, then run the
                        // extract that was held back.
                        self.access = super::Access::Unlocked;
                        if save {
                            self.run_extract_save(key);
                        } else {
                            self.run_descend_extract(key)?;
                        }
                    }
                    PromptKind::Search => {
                        // Remember the literal/regex choice for the next `/`.
                        self.search_regex = prompt.is_regex();
                        // Empty input clears the search; otherwise compile
                        // the query (literal or regex per the prompt's
                        // toggle). A bad regex flashes the parse error and
                        // leaves any active search untouched.
                        let query = if value.is_empty() {
                            Ok(None)
                        } else {
                            SearchQuery::compile(&value, prompt.is_regex()).map(Some)
                        };
                        match query {
                            Ok(query) => {
                                let f = self.frame_mut();
                                let active = f.active;
                                let target = f.modes[active].set_search(query.as_ref());
                                // owns-scroll modes return `Owned` and
                                // position themselves; flat modes return
                                // `ScrollTo(line)` for the caller.
                                if let SearchTarget::ScrollTo(line) = target {
                                    f.scroll[active] = line;
                                }
                                self.invalidate_active();
                            }
                            Err(e) => {
                                self.flash = Some(format!("invalid regex: {}", regex_reason(&e)))
                            }
                        }
                    }
                }
                Ok(true)
            }
        }
    }
}
