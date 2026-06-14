// CLI argument definitions for qork.
//
// This file is the single source of truth for the argument surface: `main.rs`
// parses it and `build.rs` pulls it in via `include!` to render the man page.
// Keep it to the argument *definitions* — command resolution lives in
// `command.rs` so the man-page build (which only needs the arg shape) doesn't
// drag in runtime logic.

use clap::Parser;

/// qork — shorten URLs from your terminal.
#[derive(Debug, Parser)]
#[command(name = "qork")]
#[command(author, version)]
#[command(about = "Shorten URLs from your terminal — the qork.me client")]
#[command(
    long_about = "qork shortens a URL by calling the qork.me API and prints the short link.\n\n\
    Paste a normal URL straight in:\n    \
    qork https://example.com/some/very/long/path\n\n\
    If the URL has spaces or shell-special characters, wrap it in quotes:\n    \
    qork \"https://example.com/a b?x=1&y=2\"\n\n\
    Use --alias for a custom code, --json for machine-readable output, and\n\
    `qork update` / `qork uninstall` to manage the install."
)]
#[command(arg_required_else_help = true)]
pub struct Cli {
    /// The URL to shorten (wrap in quotes if it has spaces or special
    /// characters). The bare words `update` and `uninstall` run those
    /// commands instead.
    #[arg(value_name = "URL")]
    pub url: Option<String>,

    /// Request a custom short code (alias) instead of a generated one
    #[arg(short = 'a', long, value_name = "ALIAS")]
    pub alias: Option<String>,

    /// Print the raw JSON response (for scripts and agents)
    #[arg(long)]
    pub json: bool,

    /// Disable colored output
    #[arg(long)]
    pub no_color: bool,

    /// API base URL to call (default: https://qork.me)
    #[arg(long, value_name = "URL", env = "QORK_API_BASE")]
    pub api_base: Option<String>,

    /// Check for a newer release and update qork in place
    #[arg(long, conflicts_with = "uninstall")]
    pub update: bool,

    /// Remove qork from this machine
    #[arg(long, conflicts_with = "update")]
    pub uninstall: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn parses_a_url_positional() {
        let cli = Cli::try_parse_from(["qork", "https://example.com"]).unwrap();
        assert_eq!(cli.url.as_deref(), Some("https://example.com"));
    }

    #[test]
    fn parses_alias_and_json() {
        let cli = Cli::try_parse_from([
            "qork",
            "https://example.com",
            "--alias",
            "my-link",
            "--json",
        ])
        .unwrap();
        assert_eq!(cli.alias.as_deref(), Some("my-link"));
        assert!(cli.json);
    }

    #[test]
    fn update_and_uninstall_flags_conflict() {
        let err = Cli::try_parse_from(["qork", "--update", "--uninstall"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn api_base_flag_parses() {
        let cli = Cli::try_parse_from([
            "qork",
            "https://example.com",
            "--api-base",
            "http://localhost:3000",
        ])
        .unwrap();
        assert_eq!(cli.api_base.as_deref(), Some("http://localhost:3000"));
    }
}
