//! Command resolution — turning parsed [`Cli`] flags into one action.
//!
//! Kept separate from `cli.rs` because `build.rs` `include!`s `cli.rs` for the
//! man page and only needs the argument shape, not this runtime logic.

use crate::cli::Cli;

/// What the parsed CLI resolves to. Centralizes the `qork update` (positional)
/// vs `qork --update` (flag) duality so `main` dispatches on one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Shorten the given URL.
    Shorten(String),
    /// Self-update from GitHub Releases.
    Update,
    /// Remove qork from the machine.
    Uninstall,
    /// No URL was given (but some flag was) — show a usage hint.
    Help,
}

impl Cli {
    /// Resolve the parsed flags + positional into a single command.
    ///
    /// `qork update` / `qork uninstall` (bare positional) are treated as the
    /// matching command — those words aren't shortenable URLs. The `--update`
    /// / `--uninstall` flags do the same thing explicitly.
    pub fn resolve(&self) -> Command {
        if self.update || self.is_bare_keyword("update") {
            return Command::Update;
        }
        if self.uninstall || self.is_bare_keyword("uninstall") {
            return Command::Uninstall;
        }
        match self.url.as_deref() {
            Some(url) => Command::Shorten(url.to_string()),
            None => Command::Help,
        }
    }

    fn is_bare_keyword(&self, keyword: &str) -> bool {
        self.url
            .as_deref()
            .is_some_and(|u| u.eq_ignore_ascii_case(keyword))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).unwrap()
    }

    #[test]
    fn url_resolves_to_shorten() {
        assert_eq!(
            parse(&["qork", "https://example.com"]).resolve(),
            Command::Shorten("https://example.com".into())
        );
    }

    #[test]
    fn bare_update_and_uninstall_resolve_to_commands() {
        assert_eq!(parse(&["qork", "update"]).resolve(), Command::Update);
        assert_eq!(parse(&["qork", "uninstall"]).resolve(), Command::Uninstall);
        // Case-insensitive.
        assert_eq!(parse(&["qork", "UPDATE"]).resolve(), Command::Update);
    }

    #[test]
    fn update_flag_resolves_to_update() {
        assert_eq!(parse(&["qork", "--update"]).resolve(), Command::Update);
    }

    #[test]
    fn flag_without_url_resolves_to_help() {
        assert_eq!(parse(&["qork", "--json"]).resolve(), Command::Help);
    }
}
