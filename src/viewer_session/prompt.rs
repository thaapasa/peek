//! Modal-prompt plumbing for the interactive viewer: the one prompt
//! slot on `ViewerState`, the [`PromptKind`] work tag that says what a
//! confirmed value means (extract save-path vs search query), and the
//! key handling that routes raw input to the open prompt.

use anyhow::Result;
use crossterm::event::KeyEvent;

use peek_foundation::extract::Extracted;
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
        self.prompt = Some((Prompt::new("Search", ""), PromptKind::Search));
    }

    pub(crate) fn handle_prompt_key(&mut self, key: KeyEvent) -> Result<bool> {
        let Some((prompt, _)) = self.prompt.as_mut() else {
            return Ok(false);
        };
        let outcome = prompt.handle_key(key);
        match outcome {
            PromptOutcome::Continue => Ok(true),
            PromptOutcome::Cancelled => {
                let (_, kind) = self.prompt.take().expect("prompt present");
                if matches!(kind, PromptKind::Extract(_)) {
                    self.flash = Some("extract cancelled".to_string());
                }
                Ok(true)
            }
            PromptOutcome::Confirmed(value) => {
                let (_, kind) = self.prompt.take().expect("prompt present");
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
                    PromptKind::Search => {
                        let query = (!value.is_empty()).then_some(value.as_str());
                        {
                            let f = self.frame_mut();
                            let active = f.active;
                            let target = f.modes[active].set_search(query);
                            // owns-scroll modes return `Owned` and
                            // position themselves; flat modes return
                            // `ScrollTo(line)` for the caller.
                            if let peek_foundation::viewer::search::SearchTarget::ScrollTo(line) =
                                target
                            {
                                f.scroll[active] = line;
                            }
                        }
                        self.invalidate_active();
                    }
                }
                Ok(true)
            }
        }
    }
}
