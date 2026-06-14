//! Self-update and uninstall for qork.
//!
//! `qork update` checks the GitHub Releases API for a newer tag and, if found,
//! re-runs the matching install path: `cargo install` when cargo is present,
//! otherwise the cargo-dist shell (Unix) / PowerShell (Windows) installer.
//! qork makes no shell-profile or registry changes, so this is intentionally
//! leaner than the tr300/wb300 updaters (no MSI/EXE/registry-origin logic).

use std::process::{Command, Stdio};

use crate::config::Config;
use crate::VERSION;

/// GitHub API endpoint for the latest release.
const RELEASES_URL: &str = "https://api.github.com/repos/QubeTX/qork/releases/latest";

/// Shell installer URL (macOS/Linux).
#[cfg(not(windows))]
const SHELL_INSTALLER: &str =
    "https://github.com/QubeTX/qork/releases/latest/download/qork-installer.sh";

/// PowerShell installer URL (Windows).
#[cfg(windows)]
const PS_INSTALLER: &str =
    "https://github.com/QubeTX/qork/releases/latest/download/qork-installer.ps1";

const CRATE_NAME: &str = "qork";
const MANUAL_INSTALL_URL: &str = "https://qork.me/install";

// ── Strategy types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateStrategy {
    Cargo,
    InstallerCurl,
    InstallerWget,
    InstallerPowerShell,
    InstallerPwsh,
}

impl UpdateStrategy {
    fn label(self) -> &'static str {
        match self {
            UpdateStrategy::Cargo => "cargo install",
            UpdateStrategy::InstallerCurl => "curl shell installer",
            UpdateStrategy::InstallerWget => "wget shell installer",
            UpdateStrategy::InstallerPowerShell => "PowerShell installer",
            UpdateStrategy::InstallerPwsh => "pwsh installer",
        }
    }

    fn json_id(self) -> &'static str {
        match self {
            UpdateStrategy::Cargo => "cargo",
            UpdateStrategy::InstallerCurl => "installer_curl",
            UpdateStrategy::InstallerWget => "installer_wget",
            UpdateStrategy::InstallerPowerShell => "installer_powershell",
            UpdateStrategy::InstallerPwsh => "installer_pwsh",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetOs {
    Unix,
    Windows,
}

#[derive(Debug)]
enum StrategyError {
    /// Required tool unavailable / could not be spawned.
    Preflight(String),
    /// Strategy launched and exited non-zero.
    Runtime(String),
}

#[derive(Debug)]
enum AttemptKind {
    Skipped,
    Failed,
}

#[derive(Debug)]
struct AttemptRecord {
    strategy: UpdateStrategy,
    kind: AttemptKind,
    message: String,
}

// ── Color helpers ──────────────────────────────────────────────────

fn green(text: &str, config: &Config) -> String {
    if config.use_colors {
        format!("\x1b[32m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn red(text: &str, config: &Config) -> String {
    if config.use_colors {
        format!("\x1b[31m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn cyan(text: &str, config: &Config) -> String {
    if config.use_colors {
        format!("\x1b[36m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

// ── Public entry points ────────────────────────────────────────────

/// Run the self-update flow. Returns a process exit code (0 = ok, 2 = error).
pub fn run(config: &Config) -> i32 {
    if config.is_json() {
        return run_json();
    }

    println!();
    println!("  {} Checking for updates...", cyan("*", config));

    let latest = match fetch_latest_version() {
        Ok(v) => v,
        Err(e) => {
            println!(
                "  {} {}",
                red("x", config),
                red(&format!("Failed to check for updates: {e}"), config)
            );
            return 2;
        }
    };

    let current = VERSION.to_string();
    if !is_newer(&current, &latest) {
        println!(
            "  {} {}",
            green("ok", config),
            green(
                &format!("Already on the latest version (v{current})"),
                config
            )
        );
        return 0;
    }

    println!(
        "  {} Update available: v{} {} v{}",
        cyan("*", config),
        current,
        cyan("->", config),
        latest
    );

    let strategies = build_strategy_list();
    if let Some(first) = strategies.first() {
        println!("  {} Updating via {}...", cyan("*", config), first.label());
    }
    println!();

    match execute_update(&latest, &strategies) {
        Ok(used) => {
            println!();
            println!(
                "  {} {}",
                green("ok", config),
                green(
                    &format!("Updated to v{latest} via {}", used.label()),
                    config
                )
            );
            0
        }
        Err(attempts) => {
            println!();
            println!(
                "  {} {}",
                red("x", config),
                red("Update failed. Strategies attempted:", config)
            );
            for record in &attempts {
                let kind = match record.kind {
                    AttemptKind::Skipped => "skipped",
                    AttemptKind::Failed => "failed",
                };
                println!(
                    "      - {} ({kind}): {}",
                    record.strategy.label(),
                    record.message
                );
            }
            println!();
            println!("  To update manually, see: {MANUAL_INSTALL_URL}");
            2
        }
    }
}

fn run_json() -> i32 {
    let latest = match fetch_latest_version() {
        Ok(v) => v,
        Err(e) => {
            println!(
                "{}",
                serde_json::json!({
                    "action": "update",
                    "success": false,
                    "message": format!("Failed to check for updates: {e}"),
                    "current_version": VERSION,
                })
            );
            return 2;
        }
    };

    let current = VERSION.to_string();
    if !is_newer(&current, &latest) {
        println!(
            "{}",
            serde_json::json!({
                "action": "update",
                "success": true,
                "message": "Already on the latest version",
                "current_version": current,
                "latest_version": latest,
                "update_available": false,
            })
        );
        return 0;
    }

    let strategies = build_strategy_list();
    match execute_update(&latest, &strategies) {
        Ok(used) => {
            println!(
                "{}",
                serde_json::json!({
                    "action": "update",
                    "success": true,
                    "message": format!("Updated from v{current} to v{latest}"),
                    "current_version": current,
                    "latest_version": latest,
                    "update_available": true,
                    "strategy": used.json_id(),
                })
            );
            0
        }
        Err(attempts) => {
            let attempts: Vec<serde_json::Value> = attempts
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "strategy": r.strategy.json_id(),
                        "result": match r.kind { AttemptKind::Skipped => "skipped", AttemptKind::Failed => "failed" },
                        "message": r.message,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::json!({
                    "action": "update",
                    "success": false,
                    "message": "Update failed; see attempts",
                    "current_version": current,
                    "latest_version": latest,
                    "update_available": true,
                    "attempts": attempts,
                })
            );
            2
        }
    }
}

/// Remove qork from the machine. qork installs only a single binary (no shell
/// profile / registry changes), so uninstall is just deleting that binary.
pub fn uninstall(config: &Config) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "{} could not locate the qork binary: {e}",
                red("error:", config)
            );
            return 2;
        }
    };

    println!();
    println!("qork is installed at:");
    println!("  {}", exe.display());
    println!();

    #[cfg(windows)]
    {
        // Windows won't let a running process delete its own executable.
        println!(
            "Windows can't delete a program while it's running. To finish removing qork, run:"
        );
        println!("  del \"{}\"", exe.display());
        println!();
        println!("If you installed with cargo, you can instead run:  cargo uninstall qork");
        0
    }

    #[cfg(not(windows))]
    {
        match std::fs::remove_file(&exe) {
            Ok(()) => {
                println!("{} qork has been removed.", green("ok", config));
                println!("(If you installed it with cargo, also run: cargo uninstall qork)");
                0
            }
            Err(e) => {
                eprintln!(
                    "{} could not remove {}: {e}",
                    red("error:", config),
                    exe.display()
                );
                eprintln!("Remove it manually, or run: cargo uninstall qork");
                2
            }
        }
    }
}

// ── Version check ──────────────────────────────────────────────────

fn classify_fetch_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, ref resp) => {
            http_status_message(code, resp.header("x-ratelimit-remaining"))
        }
        ureq::Error::Transport(t) => format!("Network error reaching GitHub: {t}"),
    }
}

/// User-facing message for an HTTP error from the releases API. Pure + testable.
fn http_status_message(code: u16, ratelimit_remaining: Option<&str>) -> String {
    if code == 403 && ratelimit_remaining == Some("0") {
        "GitHub API rate limit exceeded (unauthenticated requests are capped at 60/hour per IP). Wait for the reset and try again, or update manually.".to_string()
    } else {
        format!("GitHub API returned HTTP {code} when checking for updates")
    }
}

fn fetch_latest_version() -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(15))
        .build();

    let resp = agent
        .get(RELEASES_URL)
        .set("User-Agent", &format!("qork/{VERSION}"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(classify_fetch_error)?;

    let body: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    let tag = body["tag_name"]
        .as_str()
        .ok_or("Missing tag_name in response")?;

    Ok(tag.strip_prefix('v').unwrap_or(tag).to_string())
}

/// Strip a prerelease (`-rc.1`) or build-metadata (`+nightly.42`) suffix,
/// leaving just the `MAJOR.MINOR.PATCH` portion.
fn strip_prerelease_metadata(v: &str) -> &str {
    match v.find(|c: char| !c.is_ascii_digit() && c != '.') {
        Some(idx) => &v[..idx],
        None => v,
    }
}

/// True if `latest` is newer than `current` (semver, prerelease-aware).
fn is_newer(current: &str, latest: &str) -> bool {
    let current_stripped = strip_prerelease_metadata(current);
    let latest_stripped = strip_prerelease_metadata(latest);

    let parse =
        |v: &str| -> Vec<u64> { v.split('.').filter_map(|s| s.parse::<u64>().ok()).collect() };

    let c = parse(current_stripped);
    let l = parse(latest_stripped);

    let len = c.len().max(l.len());
    for i in 0..len {
        let cv = c.get(i).copied().unwrap_or(0);
        let lv = l.get(i).copied().unwrap_or(0);
        if lv > cv {
            return true;
        }
        if lv < cv {
            return false;
        }
    }

    // Numeric parts equal: a stable release is newer than its own prerelease.
    let current_has_suffix = current.len() != current_stripped.len();
    let latest_has_suffix = latest.len() != latest_stripped.len();
    current_has_suffix && !latest_has_suffix
}

// ── Strategy ordering & execution ──────────────────────────────────

fn order_strategies(cargo_invokable: bool, os: TargetOs) -> Vec<UpdateStrategy> {
    let mut strategies = Vec::new();
    if cargo_invokable {
        strategies.push(UpdateStrategy::Cargo);
    }
    match os {
        TargetOs::Unix => {
            strategies.push(UpdateStrategy::InstallerCurl);
            strategies.push(UpdateStrategy::InstallerWget);
        }
        TargetOs::Windows => {
            strategies.push(UpdateStrategy::InstallerPowerShell);
            strategies.push(UpdateStrategy::InstallerPwsh);
        }
    }
    strategies
}

fn current_target_os() -> TargetOs {
    if cfg!(windows) {
        TargetOs::Windows
    } else {
        TargetOs::Unix
    }
}

fn build_strategy_list() -> Vec<UpdateStrategy> {
    order_strategies(tool_exists("cargo"), current_target_os())
}

fn tool_exists(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn execute_update(
    latest: &str,
    strategies: &[UpdateStrategy],
) -> Result<UpdateStrategy, Vec<AttemptRecord>> {
    let mut attempts = Vec::new();
    for &strategy in strategies {
        match try_strategy(strategy, latest) {
            Ok(()) => return Ok(strategy),
            Err(StrategyError::Preflight(message)) => {
                eprintln!("  - skipped {}: {message}", strategy.label());
                attempts.push(AttemptRecord {
                    strategy,
                    kind: AttemptKind::Skipped,
                    message,
                });
            }
            Err(StrategyError::Runtime(message)) => {
                eprintln!("  - {} failed: {message}", strategy.label());
                attempts.push(AttemptRecord {
                    strategy,
                    kind: AttemptKind::Failed,
                    message,
                });
            }
        }
    }
    Err(attempts)
}

fn try_strategy(strategy: UpdateStrategy, latest: &str) -> Result<(), StrategyError> {
    match strategy {
        UpdateStrategy::Cargo => {
            run_command_status("cargo", &["install", CRATE_NAME, "--force"])?;
            // cargo exit 0 doesn't guarantee the running binary changed
            // (crates.io publish lag, or another qork earlier on PATH).
            verify_cargo_post_install(latest)
        }
        UpdateStrategy::InstallerCurl => try_installer_curl(),
        UpdateStrategy::InstallerWget => try_installer_wget(),
        UpdateStrategy::InstallerPowerShell => try_installer_powershell("powershell"),
        UpdateStrategy::InstallerPwsh => try_installer_powershell("pwsh"),
    }
}

fn run_command_status(launcher: &str, args: &[&str]) -> Result<(), StrategyError> {
    match Command::new(launcher).args(args).status() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(StrategyError::Preflight(format!("{launcher} not on PATH")))
        }
        Err(e) => Err(StrategyError::Preflight(format!(
            "Failed to spawn {launcher}: {e}"
        ))),
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(StrategyError::Runtime(format!(
            "{launcher} exited with code {}",
            status.code().unwrap_or(-1)
        ))),
    }
}

#[cfg(unix)]
fn try_installer_curl() -> Result<(), StrategyError> {
    if !tool_exists("curl") {
        return Err(StrategyError::Preflight("curl not on PATH".into()));
    }
    let script = format!("set -eu; curl --proto '=https' --tlsv1.2 -LsSf {SHELL_INSTALLER} | sh");
    run_command_status("sh", &["-c", &script])
}

#[cfg(not(unix))]
fn try_installer_curl() -> Result<(), StrategyError> {
    Err(StrategyError::Preflight(
        "curl installer is Unix-only".into(),
    ))
}

#[cfg(unix)]
fn try_installer_wget() -> Result<(), StrategyError> {
    if !tool_exists("wget") {
        return Err(StrategyError::Preflight("wget not on PATH".into()));
    }
    let script = format!("set -eu; wget -qO- {SHELL_INSTALLER} | sh");
    run_command_status("sh", &["-c", &script])
}

#[cfg(not(unix))]
fn try_installer_wget() -> Result<(), StrategyError> {
    Err(StrategyError::Preflight(
        "wget installer is Unix-only".into(),
    ))
}

#[cfg(windows)]
fn try_installer_powershell(launcher: &str) -> Result<(), StrategyError> {
    let script = format!("$ErrorActionPreference='Stop'; irm {PS_INSTALLER} | iex");
    run_command_status(
        launcher,
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
    )
}

#[cfg(not(windows))]
fn try_installer_powershell(_launcher: &str) -> Result<(), StrategyError> {
    Err(StrategyError::Preflight(
        "PowerShell installer is Windows-only".into(),
    ))
}

/// Re-exec the running binary with `--version`; return the parsed version
/// (last whitespace token of `qork X.Y.Z`).
fn reexec_installed_version() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let output = Command::new(&exe).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim()
        .to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Whether a freshly-installed `--version` matches the expected release tag
/// (both stripped of any prerelease/build-metadata suffix).
fn post_install_version_ok(installed: &str, expected: &str) -> bool {
    let installed_stripped = strip_prerelease_metadata(installed);
    let expected_stripped = strip_prerelease_metadata(expected);
    !installed_stripped.is_empty() && installed_stripped == expected_stripped
}

/// Confirm `cargo install qork --force` actually landed `expected`. cargo
/// reports success even when crates.io still serves the old version (publish
/// lag); on mismatch, fall through to the prebuilt installer.
fn verify_cargo_post_install(expected: &str) -> Result<(), StrategyError> {
    match reexec_installed_version() {
        Some(installed) if post_install_version_ok(&installed, expected) => Ok(()),
        Some(installed) => Err(StrategyError::Runtime(format!(
            "cargo install reported success but `qork --version` still reports v{installed} (expected v{expected}); crates.io may not have v{expected} yet — falling through to the prebuilt installer."
        ))),
        None => Err(StrategyError::Runtime(
            "cargo install reported success but `qork --version` could not be run to confirm — falling through to the prebuilt installer.".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_basic() {
        assert!(is_newer("1.0.0", "2.0.0"));
        assert!(is_newer("1.0.0", "1.1.0"));
        assert!(is_newer("1.0.0", "1.0.1"));
        assert!(!is_newer("2.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
    }

    #[test]
    fn is_newer_different_lengths() {
        assert!(is_newer("1.0", "1.0.1"));
        assert!(!is_newer("1.0.1", "1.0"));
    }

    #[test]
    fn strip_prerelease_metadata_cases() {
        assert_eq!(strip_prerelease_metadata("1.2.3-rc.1"), "1.2.3");
        assert_eq!(strip_prerelease_metadata("1.2.3+sha.abc"), "1.2.3");
        assert_eq!(strip_prerelease_metadata("1.2.3"), "1.2.3");
        assert_eq!(strip_prerelease_metadata(""), "");
    }

    #[test]
    fn is_newer_handles_prerelease_and_metadata() {
        assert!(is_newer("1.2.0", "1.2.1-rc.1"));
        assert!(is_newer("1.2.1-rc.1", "1.2.1"));
        assert!(!is_newer("1.2.1", "1.2.1+build.7"));
    }

    #[test]
    fn post_install_version_ok_cases() {
        assert!(post_install_version_ok("1.0.0", "1.0.0"));
        assert!(post_install_version_ok("1.0.0+sha.abc", "1.0.0"));
        assert!(!post_install_version_ok("1.0.0", "1.1.0"));
        assert!(!post_install_version_ok("", "1.0.0"));
    }

    #[test]
    fn http_status_message_explains_rate_limit() {
        assert!(http_status_message(403, Some("0")).contains("rate limit"));
        assert!(http_status_message(403, None).contains("HTTP 403"));
        assert!(http_status_message(500, None).contains("HTTP 500"));
    }

    #[test]
    fn unix_orders_cargo_first() {
        assert_eq!(
            order_strategies(true, TargetOs::Unix),
            vec![
                UpdateStrategy::Cargo,
                UpdateStrategy::InstallerCurl,
                UpdateStrategy::InstallerWget
            ]
        );
    }

    #[test]
    fn windows_without_cargo_prunes_cargo() {
        assert_eq!(
            order_strategies(false, TargetOs::Windows),
            vec![
                UpdateStrategy::InstallerPowerShell,
                UpdateStrategy::InstallerPwsh
            ]
        );
    }
}
