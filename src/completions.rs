//! `--completions <SHELL>`: print the shell completion script.
//!
//! Static `clap_complete` script built from the clap `Command`. Users eval it at shell start, so it
//! always matches the installed binary. The `--language` values come from the embedded syntax set.
//! They are added only here, so normal runs don't pay for loading it.

use std::io::Write as _;

use anyhow::Result;
use clap::CommandFactory;
use clap::builder::PossibleValuesParser;
use clap_complete::Shell;

use crate::cli::Args;

/// Start of the zsh script. The script registers with `compdef`, which exists only after
/// `compinit`. Without this, a user with no `compinit` before the eval line gets an error at each
/// shell start.
const ZSH_COMPINIT: &str = "(( $+functions[compdef] )) || { autoload -Uz compinit && compinit; }\n";

/// Print the completion script of `shell` to stdout.
pub fn print(shell: Shell) -> Result<()> {
    let mut buf = Vec::new();
    if shell == Shell::Zsh {
        buf.extend_from_slice(ZSH_COMPINIT.as_bytes());
    }
    clap_complete::generate(shell, &mut command(), "peek", &mut buf);
    std::io::stdout().write_all(&buf)?;
    Ok(())
}

/// The CLI command with the syntax names as `--language` values. The values only feed the
/// script: `--language` itself stays free-form, since it also takes extensions such as `rs`.
fn command() -> clap::Command {
    Args::command().mut_arg("language", |arg| {
        arg.value_parser(PossibleValuesParser::new(language_names()))
    })
}

/// Names of the visible syntaxes. Names with whitespace are left out: they need quoting, and
/// bash's word-list completion splits them. Such syntaxes still work by extension (`-L sh`).
fn language_names() -> Vec<String> {
    let tm = peek_theme::ThemeManager::new(Default::default(), Default::default());
    let mut names: Vec<String> = tm
        .syntax_set
        .syntaxes()
        .iter()
        .filter(|s| !s.hidden && !s.name.contains(char::is_whitespace))
        .map(|s| s.name.clone())
        .collect();
    names.sort_unstable();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that the script of each shell names peek's flags and the `--language` values.
    #[test]
    fn script_has_flags_and_languages() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let mut buf = Vec::new();
            clap_complete::generate(shell, &mut command(), "peek", &mut buf);
            let script = String::from_utf8(buf).unwrap();
            assert!(script.contains("image-mode"), "{shell}: no --image-mode");
            assert!(script.contains("idea-dark"), "{shell}: no theme values");
            assert!(script.contains("Rust"), "{shell}: no language values");
        }
    }
}
