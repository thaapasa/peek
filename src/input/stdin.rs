use anyhow::Result;

use peek_io::InputSource;

use crate::Args;

/// Decide the input source based on args and stdin state.
///
/// peek is a single-file viewer; the no-args + TTY case is handled in
/// `main.rs` (shows the help screen before this is called).
///
/// - `peek` with stdin piped, no args → read stdin
/// - `peek -`                         → read stdin (blocks on TTY)
/// - `peek file.rs`                   → file, stdin ignored even if piped
///
/// The actual stdin read + `/dev/tty` reopen (so the interactive event
/// loop can still read keystrokes) lives in [`peek_io::stdin`].
pub fn build_source(args: &Args) -> Result<InputSource> {
    let is_dash = args.file.as_ref().is_some_and(|p| p.as_os_str() == "-");
    let want_stdin = is_dash || args.file.is_none();

    if want_stdin {
        return peek_io::stdin::read_stdin();
    }

    Ok(InputSource::File(args.file.clone().expect("file present")))
}
