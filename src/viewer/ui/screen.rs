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
/// move-to-row + write + clear-to-EOL. This skips the full-screen
/// clear (no flash gap during animation playback) and avoids
/// rewriting unchanged regions (status-only changes touch one row).
///
/// On terminal width change the caller must `invalidate()` — a
/// byte-equal cached row in a wider terminal would still leave stale
/// cells beyond its old end without an EL pass.
pub(crate) struct ScreenBuffer {
    prev_lines: Vec<String>,
    prev_status: String,
}

impl ScreenBuffer {
    pub(crate) fn new() -> Self {
        Self {
            prev_lines: Vec::new(),
            prev_status: String::new(),
        }
    }

    /// Drop the cache so the next draw repaints every row. Called on
    /// resize.
    pub(crate) fn invalidate(&mut self) {
        self.prev_lines.clear();
        self.prev_status.clear();
    }

    /// Render `lines` (already windowed by the active mode for the
    /// current scroll) plus a status row on the bottom line. Skips
    /// rows that match the previous frame byte-for-byte. Trailing
    /// rows the previous frame populated are blanked so shrinking
    /// content doesn't leave artifacts.
    pub(crate) fn draw(
        &mut self,
        stdout: &mut io::Stdout,
        lines: &[String],
        status: &str,
        reset_bytes: &[u8],
    ) -> Result<()> {
        let (_cols, total_rows) = terminal::size().unwrap_or((80, 24));
        let rows = (total_rows as usize).saturating_sub(1);

        let end = lines.len().min(rows);
        for (i, line) in lines[..end].iter().enumerate() {
            if self.prev_lines.get(i).is_some_and(|p| p == line) {
                continue;
            }
            execute!(stdout, cursor::MoveTo(0, i as u16))?;
            stdout.write_all(line.as_bytes())?;
            // Reset before EL so clear-to-EOL paints with default bg,
            // not the line's trailing color attribute.
            stdout.write_all(reset_bytes)?;
            execute!(stdout, terminal::Clear(ClearType::UntilNewLine))?;
        }
        // Blank any trailing rows the previous frame populated. Rows
        // beyond prev_lines.len() were already empty, so skip.
        let prev_end = self.prev_lines.len().min(rows);
        for i in end..prev_end {
            execute!(stdout, cursor::MoveTo(0, i as u16))?;
            stdout.write_all(reset_bytes)?;
            execute!(stdout, terminal::Clear(ClearType::UntilNewLine))?;
        }

        let status_changed = status != self.prev_status;
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
