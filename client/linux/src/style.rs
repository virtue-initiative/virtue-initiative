//! Terminal colors for CLI output (CLIENT-020). Styling is applied only when
//! the stream is a terminal, `NO_COLOR` is unset or empty, and `TERM` isn't
//! `dumb`; otherwise every helper returns the text unchanged.

use std::io::IsTerminal;
use std::sync::OnceLock;

use crossterm::style::{StyledContent, Stylize};

#[derive(Clone, Copy)]
pub enum Stream {
    Stdout,
    Stderr,
}

fn enabled(stream: Stream) -> bool {
    static STDOUT: OnceLock<bool> = OnceLock::new();
    static STDERR: OnceLock<bool> = OnceLock::new();
    let (cell, is_terminal): (_, fn() -> bool) = match stream {
        Stream::Stdout => (&STDOUT, || std::io::stdout().is_terminal()),
        Stream::Stderr => (&STDERR, || std::io::stderr().is_terminal()),
    };
    *cell.get_or_init(|| {
        is_terminal()
            && color_allowed(
                std::env::var_os("NO_COLOR").as_deref(),
                std::env::var("TERM").ok().as_deref(),
            )
    })
}

fn color_allowed(no_color: Option<&std::ffi::OsStr>, term: Option<&str>) -> bool {
    no_color.is_none_or(|value| value.is_empty()) && term != Some("dumb")
}

fn paint(stream: Stream, text: &str, f: fn(StyledContent<&str>) -> StyledContent<&str>) -> String {
    if enabled(stream) {
        f(text.stylize()).to_string()
    } else {
        text.to_string()
    }
}

/// Section titles and prompts.
pub fn heading(text: &str) -> String {
    paint(Stream::Stdout, text, |s| s.bold())
}

/// Row labels and placeholder values such as `<none>`.
pub fn dim(text: &str) -> String {
    paint(Stream::Stdout, text, |s| s.dim())
}

/// Things that are working.
pub fn good(text: &str) -> String {
    paint(Stream::Stdout, text, |s| s.green())
}

/// Things that need the user's attention.
pub fn warn(text: &str) -> String {
    paint(Stream::Stdout, text, |s| s.yellow())
}

/// Things that are broken.
pub fn bad(text: &str) -> String {
    paint(Stream::Stdout, text, |s| s.red())
}

/// URLs and commands the user can copy.
pub fn link(text: &str) -> String {
    paint(Stream::Stdout, text, |s| s.cyan())
}

/// The `error:` prefix printed to stderr when a command fails.
pub fn error_prefix() -> String {
    paint(Stream::Stderr, "error:", |s| s.red().bold())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::color_allowed;

    // CLIENT-020: color is allowed with NO_COLOR unset or empty and a normal TERM.
    #[test]
    fn color_allowed_by_default() {
        assert!(color_allowed(None, Some("xterm-256color")));
        assert!(color_allowed(None, None));
        assert!(color_allowed(Some(OsStr::new("")), Some("xterm")));
    }

    // CLIENT-020: NO_COLOR or TERM=dumb turns color off.
    #[test]
    fn color_disabled_by_no_color_or_dumb_term() {
        assert!(!color_allowed(Some(OsStr::new("1")), Some("xterm")));
        assert!(!color_allowed(None, Some("dumb")));
    }
}
