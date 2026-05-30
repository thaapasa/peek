use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor, execute,
    terminal::{self, ClearType},
};

/// Frame buffer for the viewer's terminal output.
///
/// Caches the previous frame's content rows + status string. On each
/// `draw`, writes only rows that differ from the cache, using
/// move-to-row + clear-to-EOL + write. Clearing the row *before* the
/// write (not after) means a row that paints fewer cells than last
/// frame — or a stray cursor-moving control char that skips cells
/// without painting them — can never leave previous-frame content
/// showing through. This skips the full-screen clear (no flash gap
/// during animation playback) and avoids rewriting unchanged regions
/// (status-only changes touch one row).
///
/// On terminal width change the caller must `invalidate()` — a
/// byte-equal cached row in a wider terminal would still leave stale
/// cells beyond its old end without an EL pass.
pub(crate) struct ScreenBuffer {
    prev_lines: Vec<String>,
    prev_status: String,
    /// Set by `invalidate`. Next `draw` repaints every row, blanks
    /// every cell up to the terminal height, and forces a status
    /// rewrite — so a resize, theme cycle, or stack push/pop never
    /// leaves stale content from the previous frame on screen.
    force_redraw: bool,
}

impl ScreenBuffer {
    pub(crate) fn new() -> Self {
        Self {
            prev_lines: Vec::new(),
            prev_status: String::new(),
            force_redraw: true,
        }
    }

    /// Mark the screen for a full repaint on the next `draw`.
    pub(crate) fn invalidate(&mut self) {
        self.force_redraw = true;
    }

    /// Render `lines` (already windowed by the active mode for the
    /// current scroll) plus a status row on the bottom line. Skips
    /// rows that match the previous frame byte-for-byte. Trailing
    /// rows from the previous frame are blanked so shrinking content
    /// doesn't leave artifacts; on `force_redraw`, every row up to
    /// the full terminal height is blanked first so swap-in of a
    /// completely different document (stack push/pop) starts clean.
    pub(crate) fn draw(
        &mut self,
        stdout: &mut io::Stdout,
        lines: &[String],
        status: &str,
        reset_bytes: &[u8],
    ) -> Result<()> {
        let (_cols, total_rows) = terminal::size().unwrap_or((80, 24));
        let rows = (total_rows as usize).saturating_sub(1);

        let force = std::mem::take(&mut self.force_redraw);
        // On force, blank every row first so anything not overwritten
        // by content is cleared. Without this, a smaller new frame
        // (or a different layout entirely) leaves the old frame's
        // tail visible.
        let blank_through = if force {
            self.prev_lines.clear();
            self.prev_status.clear();
            rows
        } else {
            self.prev_lines.len().min(rows)
        };

        let end = lines.len().min(rows);
        for (i, line) in lines[..end].iter().enumerate() {
            if !force && self.prev_lines.get(i).is_some_and(|p| p == line) {
                continue;
            }
            execute!(stdout, cursor::MoveTo(0, i as u16))?;
            // Clear the whole row to default bg before writing it. Reset
            // first so the clear paints with default bg, not a leftover
            // color attribute. Pre-clearing (rather than a trailing
            // clear-to-EOL) guarantees no stale cell survives even if the
            // new line is shorter or contains a cursor-jumping control.
            stdout.write_all(reset_bytes)?;
            execute!(stdout, terminal::Clear(ClearType::UntilNewLine))?;
            stdout.write_all(line.as_bytes())?;
            // Trailing reset so the line's last color attribute doesn't
            // bleed into the next row's pre-clear or the status line.
            stdout.write_all(reset_bytes)?;
        }
        for i in end..blank_through {
            execute!(stdout, cursor::MoveTo(0, i as u16))?;
            stdout.write_all(reset_bytes)?;
            execute!(stdout, terminal::Clear(ClearType::UntilNewLine))?;
        }

        let status_changed = force || status != self.prev_status;
        if status_changed {
            execute!(stdout, cursor::MoveTo(0, total_rows.saturating_sub(1)))?;
            stdout.write_all(reset_bytes)?;
            execute!(stdout, terminal::Clear(ClearType::UntilNewLine))?;
            stdout.write_all(status.as_bytes())?;
        }

        stdout.flush()?;

        self.prev_lines.clear();
        self.prev_lines.extend_from_slice(&lines[..end]);
        if status_changed {
            self.prev_status.clear();
            self.prev_status.push_str(status);
        }

        Ok(())
    }
}
