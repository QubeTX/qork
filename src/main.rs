//! qork — shorten URLs from your terminal.
//!
//! `qork <url>` prints the short link to stdout (just the URL, so it pipes
//! cleanly). `--json` prints the raw API envelope. `qork update` /
//! `qork uninstall` manage the install. Errors go to stderr with exit code 2.

use std::io::IsTerminal;

use clap::Parser;

use qork::cli::Cli;
use qork::config::Config;
use qork::{shorten, update, Command};

fn main() {
    let cli = Cli::parse();

    // Color only when stdout is a real terminal (keeps pipes/CI output clean)
    // and the user didn't opt out. The plain short URL is never colored.
    let config = Config::new()
        .with_colors(!cli.no_color && std::io::stdout().is_terminal())
        .with_json(cli.json)
        .with_api_base(cli.api_base.clone());

    let code = match cli.resolve() {
        Command::Update => update::run(&config),
        Command::Uninstall => update::uninstall(&config),
        Command::Shorten(url) => run_shorten(&url, cli.alias.as_deref(), &config),
        Command::Help => {
            eprintln!(
                "error: a URL is required.\n\nUsage: qork <URL>\nRun 'qork --help' for more information."
            );
            2
        }
    };

    std::process::exit(code);
}

fn run_shorten(url: &str, alias: Option<&str>, config: &Config) -> i32 {
    match shorten(url, alias, config) {
        Ok(link) => {
            if config.is_json() {
                // Raw server envelope — agent/script friendly.
                println!("{}", link.raw);
            } else {
                // stdout is exactly the short URL, so `qork ... | pbcopy` works.
                println!("{}", link.display_url());
            }
            0
        }
        Err(e) => {
            if config.is_json() {
                println!(
                    "{}",
                    serde_json::json!({ "ok": false, "error": e.to_string() })
                );
            } else {
                let prefix = if config.use_colors {
                    "\x1b[31merror:\x1b[0m"
                } else {
                    "error:"
                };
                eprintln!("{prefix} {e}");
            }
            2
        }
    }
}
