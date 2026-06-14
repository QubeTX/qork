//! qork — shorten URLs from your terminal.
//!
//! `qork <url>` prints the short link to stdout (just the URL, so it pipes
//! cleanly). `--json` prints the raw API envelope. Before shortening, qork
//! sanity-checks that the argument is a real, reachable link (skip with
//! `--no-check`). `qork update` / `qork uninstall` manage the install;
//! `qork help` prints help. Errors go to stderr with exit code 2.

use std::io::IsTerminal;

use clap::{CommandFactory, Parser};

use qork::check::{self, Verdict};
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
        Command::Uninstall => update::uninstall(&config, cli.yes, std::io::stdin().is_terminal()),
        Command::Shorten(url) => run_shorten(&url, cli.alias.as_deref(), !cli.no_check, &config),
        Command::Help => {
            // `qork help` is documentation — full help to stdout, exit 0.
            let _ = Cli::command().print_long_help();
            println!();
            0
        }
        Command::MissingUrl => {
            eprintln!(
                "error: a URL is required.\n\nUsage: qork <URL>\nRun 'qork help' for more information."
            );
            2
        }
    };

    std::process::exit(code);
}

fn run_shorten(url: &str, alias: Option<&str>, check_enabled: bool, config: &Config) -> i32 {
    // 1. Offline sanity (empty / unquoted spaces) — never touches the network.
    if let Some(msg) = qork::api::quick_url_check(url) {
        return fail(&msg, config);
    }

    // 2. Pre-shorten check: is this a real, live link? (skipped by --no-check)
    //    Refuses random pasted text (offline) and dead/never-valid links (a
    //    quick HEAD ping); auth walls, 5xx, and transient blips still proceed.
    match check::check(url, check_enabled) {
        Verdict::Ok => {}
        Verdict::NotUrl(msg) => return fail(&msg, config),
        Verdict::Dead(status) => {
            return fail(
                &format!(
                    "that link looks dead — it returned HTTP {status}. Re-run with --no-check to shorten it anyway."
                ),
                config,
            );
        }
        Verdict::NoSuchHost => {
            return fail(
                "couldn't resolve that host — the URL looks mistyped or was never valid. Re-run with --no-check to shorten it anyway.",
                config,
            );
        }
    }

    // 3. Shorten.
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
        Err(e) => fail(&e.to_string(), config),
    }
}

/// Emit an error (JSON when `--json`, else colored plain text on stderr) and
/// return process exit code 2.
fn fail(message: &str, config: &Config) -> i32 {
    if config.is_json() {
        println!("{}", serde_json::json!({ "ok": false, "error": message }));
    } else {
        let prefix = if config.use_colors {
            "\x1b[31merror:\x1b[0m"
        } else {
            "error:"
        };
        eprintln!("{prefix} {message}");
    }
    2
}
