//! Self-update and uninstall for qork.
//!
//! `qork update` checks the GitHub Releases API for a newer tag and, if found,
//! dispatches to an installer strategy that matches how this binary was
//! installed. On Windows (v1.0.1+), the four first-class installers (MSI
//! Global, MSI Corporate, EXE Global, EXE Corporate) write a
//! `HKCU\Software\Qork\InstallSource` registry marker, which
//! `detect_install_origin()` reads to pick the matching MSI/EXE for an
//! in-place upgrade. A path-based fallback handles the `cargo install` /
//! shell / PowerShell installer path that doesn't write a marker.
//!
//! qork installs ONLY a single binary on PATH (no shell-profile changes, no
//! alias, no auto-run, no migrate-cleanup), so the installer strategies here
//! are deliberately leaner than tr300's: they install qork.exe, add it to
//! PATH, and write the install-origin marker — nothing else.

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

/// Global (perMachine) MSI installer URL.
#[cfg(windows)]
const MSI_GLOBAL_URL: &str =
    "https://github.com/QubeTX/qork/releases/latest/download/qork-x86_64-pc-windows-msvc.msi";

/// Corporate (perUser) MSI installer URL.
#[cfg(windows)]
const MSI_CORPORATE_URL: &str = "https://github.com/QubeTX/qork/releases/latest/download/qork-x86_64-pc-windows-msvc-corporate.msi";

/// Global (perMachine) EXE installer URL (Inno Setup).
#[cfg(windows)]
const EXE_GLOBAL_URL: &str =
    "https://github.com/QubeTX/qork/releases/latest/download/qork-x86_64-pc-windows-msvc-setup.exe";

/// Corporate (perUser) EXE installer URL (Inno Setup).
#[cfg(windows)]
const EXE_CORPORATE_URL: &str = "https://github.com/QubeTX/qork/releases/latest/download/qork-x86_64-pc-windows-msvc-corporate-setup.exe";

const CRATE_NAME: &str = "qork";
const MANUAL_INSTALL_URL: &str = "https://qork.me/install";

// ── Strategy types ─────────────────────────────────────────────────

/// Ordered candidate strategies for updating the binary.
///
/// For Windows MSI / EXE installer strategies (v1.0.1+), the runner picks
/// exactly one strategy based on `detect_install_origin()` and does NOT fall
/// back to a different installer type on failure — re-running a different
/// product would create coexistence problems (two Add/Remove Programs
/// entries, PATH ordering decides which wins). For `cargo install` / shell
/// installer users, the legacy probe-and-retry chain runs as before.
// The four MSI/EXE variants are only ever constructed by build_strategy_list()
// inside its #[cfg(windows)] block, so on non-Windows targets the dead_code
// lint flags them as never-constructed. The variants still need to exist on
// every platform so the label()/json_id() match arms stay exhaustive and the
// try_strategy() dispatch arms compile. cfg_attr keeps Windows clippy strict
// (so missing wiring on Windows still trips the lint) while silencing it on
// Linux/macOS.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateStrategy {
    Cargo,
    InstallerCurl,
    InstallerWget,
    InstallerPowerShell,
    InstallerPwsh,
    /// Re-runs the Global perMachine MSI (UAC required).
    MsiGlobal,
    /// Re-runs the Corporate perUser MSI (no UAC required).
    MsiCorporate,
    /// Re-runs the Global perMachine Inno Setup EXE (UAC required).
    ExeGlobal,
    /// Re-runs the Corporate perUser Inno Setup EXE (no UAC required).
    ExeCorporate,
}

impl UpdateStrategy {
    fn label(self) -> &'static str {
        match self {
            UpdateStrategy::Cargo => "cargo install",
            UpdateStrategy::InstallerCurl => "curl shell installer",
            UpdateStrategy::InstallerWget => "wget shell installer",
            UpdateStrategy::InstallerPowerShell => "PowerShell installer",
            UpdateStrategy::InstallerPwsh => "pwsh installer",
            UpdateStrategy::MsiGlobal => "Global MSI installer",
            UpdateStrategy::MsiCorporate => "Corporate MSI installer",
            UpdateStrategy::ExeGlobal => "Global EXE installer",
            UpdateStrategy::ExeCorporate => "Corporate EXE installer",
        }
    }

    fn json_id(self) -> &'static str {
        match self {
            UpdateStrategy::Cargo => "cargo",
            UpdateStrategy::InstallerCurl => "installer_curl",
            UpdateStrategy::InstallerWget => "installer_wget",
            UpdateStrategy::InstallerPowerShell => "installer_powershell",
            UpdateStrategy::InstallerPwsh => "installer_pwsh",
            UpdateStrategy::MsiGlobal => "msi_global",
            UpdateStrategy::MsiCorporate => "msi_corporate",
            UpdateStrategy::ExeGlobal => "exe_global",
            UpdateStrategy::ExeCorporate => "exe_corporate",
        }
    }
}

/// Where this binary was installed from, on Windows.
///
/// Determines which installer `qork update` downloads and re-runs for an
/// in-place upgrade. First-class installers (the four MSI/EXE variants) write
/// a `HKCU\Software\Qork\InstallSource` registry marker on install;
/// `detect_install_origin()` reads that marker. A path-based fallback covers
/// the `cargo install` / shell / PowerShell installer path that doesn't write
/// a marker.
#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallOrigin {
    /// `C:\Program Files\qork\bin\qork.exe`, installed from wix/main.wxs.
    MsiGlobal,
    /// `%LocalAppData%\Programs\qork\bin\qork.exe`, from wix-corporate/corporate.wxs.
    MsiCorporate,
    /// `C:\Program Files\qork\bin\qork.exe`, from inno/global.iss.
    ExeGlobal,
    /// `%LocalAppData%\Programs\qork\bin\qork.exe`, from inno/corporate.iss.
    ExeCorporate,
    /// `~\.cargo\bin\qork.exe` — installed via `cargo install` or the
    /// cargo-dist PowerShell installer. Uses the legacy strategy chain.
    CargoOrInstaller,
    /// Couldn't determine origin (custom install location, portable use,
    /// etc.). Treated like `CargoOrInstaller` so the legacy chain runs.
    Unknown,
}

#[cfg(windows)]
impl InstallOrigin {
    /// String form for JSON output. Matches the registry marker values written
    /// by the installers; `cargo-or-installer` / `unknown` are synthesized by
    /// the path-based fallback.
    fn json_id(self) -> &'static str {
        match self {
            InstallOrigin::MsiGlobal => "msi-global",
            InstallOrigin::MsiCorporate => "msi-corporate",
            InstallOrigin::ExeGlobal => "exe-global",
            InstallOrigin::ExeCorporate => "exe-corporate",
            InstallOrigin::CargoOrInstaller => "cargo-or-installer",
            InstallOrigin::Unknown => "unknown",
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
            let mut payload = serde_json::json!({
                "action": "update",
                "success": false,
                "message": format!("Failed to check for updates: {e}"),
                "current_version": VERSION,
            });
            inject_install_origin(&mut payload);
            println!("{payload}");
            return 2;
        }
    };

    let current = VERSION.to_string();
    if !is_newer(&current, &latest) {
        let mut payload = serde_json::json!({
            "action": "update",
            "success": true,
            "message": "Already on the latest version",
            "current_version": current,
            "latest_version": latest,
            "update_available": false,
        });
        inject_install_origin(&mut payload);
        println!("{payload}");
        return 0;
    }

    let strategies = build_strategy_list();
    match execute_update(&latest, &strategies) {
        Ok(used) => {
            let mut payload = serde_json::json!({
                "action": "update",
                "success": true,
                "message": format!("Updated from v{current} to v{latest}"),
                "current_version": current,
                "latest_version": latest,
                "update_available": true,
                "strategy": used.json_id(),
            });
            inject_install_origin(&mut payload);
            println!("{payload}");
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
            let mut payload = serde_json::json!({
                "action": "update",
                "success": false,
                "message": "Update failed; see attempts",
                "current_version": current,
                "latest_version": latest,
                "update_available": true,
                "attempts": attempts,
            });
            inject_install_origin(&mut payload);
            println!("{payload}");
            2
        }
    }
}

/// Add a top-level `install_origin` field to the JSON payload on Windows.
/// On other platforms this is a no-op — the field is Windows-only because
/// install-origin only meaningfully varies on Windows (where users have a
/// choice of MSI vs EXE installer, perMachine vs perUser scope).
#[cfg(windows)]
fn inject_install_origin(payload: &mut serde_json::Value) {
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "install_origin".to_string(),
            serde_json::Value::String(detect_install_origin().json_id().to_string()),
        );
    }
}

#[cfg(not(windows))]
fn inject_install_origin(_payload: &mut serde_json::Value) {
    // No-op on non-Windows; the field would always be the same value.
}

/// Fully remove qork from this machine — origin-aware on Windows, mirroring
/// tr300 / wb300.
///
/// qork installs ONLY a single binary (no shell-profile edits, no alias, no
/// auto-run), so a complete uninstall removes: the binary, its PATH entry, the
/// Add/Remove-Programs entry (installer installs), the `HKCU\Software\Qork`
/// marker, and the cargo-dist receipt — nothing else.
///
/// Strategy per channel (Windows):
/// - **MSI Global / MSI Corporate** → find qork's Add/Remove-Programs
///   `UninstallString` in the registry and run `msiexec /x {ProductCode}
///   /passive /norestart`. perMachine triggers UAC; perUser is silent. This
///   removes the files, the PATH entry, and the ARP entry.
/// - **EXE Global / EXE Corporate** → run the Inno Setup uninstaller (the
///   `QuietUninstallString` / `UninstallString` for qork's AppId) with
///   `/SILENT /SUPPRESSMSGBOXES /NORESTART`.
/// - **Cargo / shell / PowerShell installer / Unknown** → the running exe
///   can't delete itself in place on Windows, so spawn a DETACHED helper that
///   waits for this process to exit, then deletes the binary; also delete the
///   cargo-dist receipt and best-effort-clean the `~\.cargo\bin` PATH entry the
///   installer added.
///
/// On Unix a running binary CAN be unlinked, so we remove it directly along
/// with the cargo-dist receipt.
///
/// `assume_yes` skips the confirmation prompt. `stdin_is_tty` gates the prompt:
/// on an interactive terminal qork asks `Remove qork from this system? [y/N]`;
/// when stdin is NOT a TTY and `--yes` wasn't given, qork refuses (so scripts
/// are explicit). Returns a process exit code (0 = ok, 1 = aborted/refused,
/// 2 = error).
pub fn uninstall(config: &Config, assume_yes: bool, stdin_is_tty: bool) -> i32 {
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

    if !assume_yes {
        if !stdin_is_tty {
            eprintln!(
                "{} refusing to uninstall without confirmation: stdin is not a terminal.",
                red("error:", config)
            );
            eprintln!("Re-run with --yes (or -y) to remove qork non-interactively.");
            return 1;
        }
        if !confirm_uninstall() {
            println!("Aborted — nothing was removed.");
            return 1;
        }
    }

    #[cfg(windows)]
    {
        windows_uninstall(&exe, config)
    }

    #[cfg(not(windows))]
    {
        unix_uninstall(&exe, config)
    }
}

/// Prompt `Remove qork from this system? [y/N]` on stdin; only an explicit
/// `y` / `yes` (case-insensitive) confirms. Any read error counts as "no".
fn confirm_uninstall() -> bool {
    use std::io::{BufRead, Write};
    print!("Remove qork from this system? [y/N]: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return false;
    }
    let answer = line.trim().to_ascii_lowercase();
    answer == "y" || answer == "yes"
}

// ── Unix uninstall ─────────────────────────────────────────────────

/// Remove the binary (a running Unix binary CAN be unlinked) and the cargo-dist
/// receipt. Note the PATH entry the installer added (we don't edit shell
/// profiles).
#[cfg(not(windows))]
fn unix_uninstall(exe: &std::path::Path, config: &Config) -> i32 {
    let mut ok = true;

    match std::fs::remove_file(exe) {
        Ok(()) => println!("{} removed binary: {}", green("ok", config), exe.display()),
        Err(e) => {
            eprintln!(
                "{} could not remove {}: {e}",
                red("error:", config),
                exe.display()
            );
            eprintln!("Remove it manually, or run: cargo uninstall qork");
            ok = false;
        }
    }

    remove_cargo_dist_receipt(config);

    println!();
    println!("Note: the installer added qork's directory to your PATH (in your shell");
    println!("profile or the cargo env file). qork doesn't edit shell profiles, so if");
    println!("you want that line gone, remove it by hand.");

    if ok {
        println!();
        println!("{} qork has been removed.", green("ok", config));
        0
    } else {
        2
    }
}

/// Delete the cargo-dist install receipt, if present. The shell installer
/// writes it to `$HOME/.config/qork/qork-receipt.json`; the PowerShell
/// installer to `%LOCALAPPDATA%\qork\qork-receipt.json` (cargo-dist
/// convention). Best-effort — a missing receipt (cargo / MSI / EXE installs
/// never write one) is not an error.
fn remove_cargo_dist_receipt(config: &Config) {
    for path in cargo_dist_receipt_paths() {
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => println!(
                    "{} removed install receipt: {}",
                    green("ok", config),
                    path.display()
                ),
                Err(e) => eprintln!(
                    "  (couldn't remove install receipt {}: {e} — harmless, remove by hand)",
                    path.display()
                ),
            }
        }
    }
}

/// Candidate cargo-dist receipt paths for this platform. Returns every location
/// a shell/PowerShell installer might have written to (we don't know which ran,
/// so we check all and delete whatever exists).
fn cargo_dist_receipt_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            paths.push(
                std::path::PathBuf::from(local)
                    .join("qork")
                    .join("qork-receipt.json"),
            );
        }
    }
    #[cfg(not(windows))]
    {
        // Shell installer: $XDG_CONFIG_HOME/qork or ~/.config/qork.
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            paths.push(
                std::path::PathBuf::from(xdg)
                    .join("qork")
                    .join("qork-receipt.json"),
            );
        } else if let Some(home) = std::env::var_os("HOME") {
            paths.push(
                std::path::PathBuf::from(home)
                    .join(".config")
                    .join("qork")
                    .join("qork-receipt.json"),
            );
        }
    }
    paths
}

// ── Windows uninstall ──────────────────────────────────────────────

/// Origin-aware Windows uninstall. Branches on `detect_install_origin()`:
/// installer installs (MSI/EXE) run their recorded uninstaller; cargo / shell /
/// PowerShell / unknown installs schedule a detached self-delete of the binary.
/// Always best-effort-deletes the `HKCU\Software\Qork` marker last.
#[cfg(windows)]
fn windows_uninstall(exe: &std::path::Path, config: &Config) -> i32 {
    let origin = detect_install_origin();

    let result = match origin {
        InstallOrigin::MsiGlobal
        | InstallOrigin::MsiCorporate
        | InstallOrigin::ExeGlobal
        | InstallOrigin::ExeCorporate => match find_uninstall_entry() {
            Some(entry) => {
                println!(
                    "  {} Launching the uninstaller for \"{}\"...",
                    cyan("*", config),
                    entry.display_name
                );
                match launch_uninstaller(&entry) {
                    Ok(()) => Ok(format!(
                        "The {} uninstaller is running; it will remove the binary, its PATH entry, and the Add/Remove Programs entry.",
                        if entry.is_msi { "Windows Installer" } else { "Inno Setup" }
                    )),
                    Err(e) => Err(format!(
                        "found the Add/Remove Programs entry but couldn't launch its uninstaller: {e}\n         Uninstall via Settings -> Apps -> Installed apps, or run:\n           {}",
                        entry.command
                    )),
                }
            }
            None => Err(format!(
                "no Add/Remove Programs entry was found for qork (install origin: {}).\n         Uninstall via Settings -> Apps -> Installed apps.",
                origin.json_id()
            )),
        },
        InstallOrigin::CargoOrInstaller | InstallOrigin::Unknown => {
            schedule_self_delete(exe, config)
        }
    };

    // Best-effort: remove the HKCU\Software\Qork marker key regardless of path.
    // The installer uninstallers (MSI/Inno) clean their own InstallSource value,
    // but a cargo/script install leaves no marker, and a belt-and-braces delete
    // here makes `qork uninstall` idempotent.
    remove_install_marker(config);

    match result {
        Ok(action) => {
            println!();
            println!("  {} {}", green("ok", config), green(&action, config));
            0
        }
        Err(message) => {
            eprintln!();
            eprintln!("{} {}", red("error:", config), red(&message, config));
            2
        }
    }
}

/// Schedule a detached `cmd /c` helper that waits for THIS process to exit, then
/// deletes the binary. Windows refuses to delete a running executable, so the
/// helper must outlive us. Also deletes the cargo-dist receipt synchronously
/// (it's a plain file, not in use). Returns the success/diagnostic message.
#[cfg(windows)]
fn schedule_self_delete(exe: &std::path::Path, config: &Config) -> Result<String, String> {
    // Receipt deletion is safe to do now — it's not the running image.
    remove_cargo_dist_receipt(config);
    // Best-effort: drop qork's line from the cargo env file the installer wrote.
    clean_cargo_env_path_entry(config);

    let script = self_delete_command(&exe.to_string_lossy());

    use std::os::windows::process::CommandExt;
    // DETACHED_PROCESS (0x00000008): the helper has no console window.
    // CREATE_NEW_PROCESS_GROUP (0x00000200): it doesn't inherit our process
    //   group, so a Ctrl-C in the terminal after we exit won't kill it.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    match Command::new("cmd")
        .arg("/c")
        .arg(&script)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => Ok(format!(
            "Scheduled removal of {} — it will be deleted within a few seconds of this process exiting.",
            exe.display()
        )),
        Err(e) => Err(format!(
            "couldn't schedule the deferred delete ({e}). Windows can't delete a running program, so finish the removal manually after qork exits:\n           del /f /q \"{}\"",
            exe.display()
        )),
    }
}

/// Build the detached `cmd /c` command that waits for the running qork to exit
/// and then deletes the binary. `ping 127.0.0.1 -n 3` is a portable ~2s sleep
/// (no `timeout` dependency, which can fail when stdin is redirected). Pure +
/// unit-testable so the quoting/structure is pinned without spawning anything.
#[cfg(any(target_os = "windows", test))]
fn self_delete_command(exe_path: &str) -> String {
    format!("(ping 127.0.0.1 -n 3 >nul & del /f /q \"{exe_path}\")")
}

/// Best-effort: remove qork's `~\.cargo\bin` entry from the cargo-dist env file,
/// but ONLY the line qork added — never cargo's own PATH setup. The cargo-dist
/// shell installer appends a source line; on Windows the PowerShell installer
/// edits the user PATH env var directly (handled by Windows itself on cargo
/// uninstall). This is intentionally conservative: if we can't be sure a line is
/// qork's, we leave it. Today this is a documentation no-op stub on Windows —
/// the PowerShell installer's PATH edit and the binary delete cover the real
/// cases — but it keeps the call site honest about intent.
#[cfg(windows)]
fn clean_cargo_env_path_entry(_config: &Config) {
    // The cargo-dist PowerShell installer adds `%USERPROFILE%\.cargo\bin` to the
    // user PATH only if it wasn't already there. We don't remove it because
    // cargo itself relies on that directory being on PATH, and qork can't tell
    // whether qork or cargo put it there. Removing it could break `cargo
    // install`'d tools. Leaving the (now-empty-of-qork) directory on PATH is
    // harmless. Documented here so the omission is deliberate, not forgotten.
}

/// Remove the `HKCU\Software\Qork` install-origin marker key. Best-effort: a
/// missing key (cargo/script install) is not an error.
#[cfg(windows)]
fn remove_install_marker(config: &Config) {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match hkcu.delete_subkey_all("Software\\Qork") {
        Ok(()) => println!(
            "  {} removed registry marker HKCU\\Software\\Qork",
            green("ok", config)
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("  (couldn't remove HKCU\\Software\\Qork: {e} — harmless)"),
    }
}

/// One Add/Remove Programs entry for qork.
#[cfg(windows)]
struct UninstallEntry {
    display_name: String,
    /// The recorded uninstall command. Prefer `QuietUninstallString` (Inno
    /// records one) over `UninstallString`.
    command: String,
    quiet: bool,
    is_msi: bool,
}

/// Scan the uninstall registry roots (per-machine 64-bit, per-machine 32-bit
/// via WOW6432Node, and per-user) for qork's Add/Remove Programs entry. Matches
/// the exact product DisplayNames the four installers register: `qork` (Global
/// MSI/EXE) and `qork (Corporate Edition)` (Corporate MSI/EXE).
#[cfg(windows)]
fn find_uninstall_entry() -> Option<UninstallEntry> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    const SUBKEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    const SUBKEY_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
    let roots = [
        (RegKey::predef(HKEY_LOCAL_MACHINE), SUBKEY),
        (RegKey::predef(HKEY_LOCAL_MACHINE), SUBKEY_32),
        (RegKey::predef(HKEY_CURRENT_USER), SUBKEY),
    ];

    for (root, subkey) in roots {
        let Ok(uninstall) = root.open_subkey(subkey) else {
            continue;
        };
        for name in uninstall.enum_keys().flatten() {
            let Ok(entry) = uninstall.open_subkey(&name) else {
                continue;
            };
            let Ok(display): Result<String, _> = entry.get_value("DisplayName") else {
                continue;
            };
            if !is_qork_display_name(&display) {
                continue;
            }
            let quiet_cmd: Option<String> = entry.get_value("QuietUninstallString").ok();
            let loud_cmd: Option<String> = entry.get_value("UninstallString").ok();
            let (command, quiet) = match (quiet_cmd, loud_cmd) {
                (Some(q), _) if !q.trim().is_empty() => (q, true),
                (_, Some(l)) if !l.trim().is_empty() => (l, false),
                _ => continue,
            };
            let is_msi = command.to_lowercase().contains("msiexec");
            return Some(UninstallEntry {
                display_name: display,
                command,
                quiet,
                is_msi,
            });
        }
    }
    None
}

/// Exact-match qork's product DisplayName. A substring match could launch some
/// unrelated product's uninstaller, so we only accept the two names the
/// installers register (case-insensitively).
#[cfg(any(target_os = "windows", test))]
fn is_qork_display_name(display: &str) -> bool {
    let lower = display.trim().to_lowercase();
    lower == "qork" || lower == "qork (corporate edition)"
}

/// Launch the recorded uninstaller, detached, so it outlives this (running)
/// process and can delete the binary. MSI commands are rewritten to `/X` and run
/// `/passive /norestart`; Inno commands run `/SILENT /SUPPRESSMSGBOXES
/// /NORESTART` (added only when the registry didn't already record a quiet
/// command).
#[cfg(windows)]
fn launch_uninstaller(entry: &UninstallEntry) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    // Windows Installer often records `MsiExec.exe /I{GUID}` (maintenance/repair
    // mode) as the UninstallString. Always rewrite to `/X` (uninstall).
    let command = if entry.is_msi {
        msi_uninstall_command(&entry.command)
    } else {
        entry.command.clone()
    };
    let (program, mut args) = split_command_line(&command);

    if entry.is_msi {
        // /passive: progress UI, no prompts (we already confirmed in the
        // terminal). /norestart: don't reboot for a file replace. For the
        // perMachine MSI this triggers UAC; the perUser MSI is silent.
        args.push("/passive".to_string());
        args.push("/norestart".to_string());
    } else if !entry.quiet {
        // Inno: drive it silently when the registry didn't record a quiet
        // command. (When it did, QuietUninstallString already carries /SILENT.)
        args.push("/SILENT".to_string());
        args.push("/SUPPRESSMSGBOXES".to_string());
        args.push("/NORESTART".to_string());
    }

    let mut cmd = Command::new(program);
    for a in &args {
        cmd.arg(a);
    }
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()
        .map(|_| ())
}

/// Rewrite an msiexec command line into uninstall (`/X`) mode by extracting the
/// `{GUID}` product code and rebuilding as `MsiExec.exe /X{GUID}`. Registry
/// UninstallStrings come as `/I{GUID}`, `/X{GUID}`, `/x {GUID}`, etc. Falls back
/// to the original string when no GUID is present. Pure + unit-tested.
#[cfg(any(target_os = "windows", test))]
fn msi_uninstall_command(command: &str) -> String {
    let (start, end) = match (command.find('{'), command.rfind('}')) {
        (Some(s), Some(e)) if e > s => (s, e),
        _ => return command.to_string(),
    };
    format!("MsiExec.exe /X{}", &command[start..=end])
}

/// Split a registry command line into (program, args): a leading quoted span or
/// the text up to the first space, then whitespace-separated arguments. Known
/// limit: a QUOTED argument after the program (e.g. `/LOG="a b.log"`) would be
/// split on its inner space — none of qork's four installers record one. Pure +
/// unit-tested.
#[cfg(any(target_os = "windows", test))]
fn split_command_line(command: &str) -> (String, Vec<String>) {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        match rest.split_once('"') {
            Some((program, tail)) => (
                program.to_string(),
                tail.split_whitespace().map(str::to_string).collect(),
            ),
            None => (rest.to_string(), Vec::new()),
        }
    } else {
        match command.split_once(' ') {
            Some((program, tail)) => (
                program.to_string(),
                tail.split_whitespace().map(str::to_string).collect(),
            ),
            None => (command.to_string(), Vec::new()),
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
    #[cfg(windows)]
    {
        // For first-class Windows installs, dispatch to a single
        // matching-installer strategy. Don't cross-fall-back to a different
        // installer type — running a different product would create
        // coexistence problems (two ARP entries, PATH ordering wins).
        match detect_install_origin() {
            InstallOrigin::MsiGlobal => return vec![UpdateStrategy::MsiGlobal],
            InstallOrigin::MsiCorporate => return vec![UpdateStrategy::MsiCorporate],
            InstallOrigin::ExeGlobal => return vec![UpdateStrategy::ExeGlobal],
            InstallOrigin::ExeCorporate => return vec![UpdateStrategy::ExeCorporate],
            InstallOrigin::CargoOrInstaller | InstallOrigin::Unknown => {
                // Fall through to the legacy cargo/PS chain.
            }
        }
    }
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
        UpdateStrategy::MsiGlobal => try_msi_install(msi_global_url(), latest),
        UpdateStrategy::MsiCorporate => try_msi_install(msi_corporate_url(), latest),
        UpdateStrategy::ExeGlobal => try_exe_install(exe_global_url(), latest),
        UpdateStrategy::ExeCorporate => try_exe_install(exe_corporate_url(), latest),
    }
}

// URL accessors are #[cfg(windows)] under the hood but we expose them
// uniformly so the dispatch arms above don't need their own #[cfg] gates —
// keeps the match exhaustive on all platforms.
#[cfg(windows)]
fn msi_global_url() -> &'static str {
    MSI_GLOBAL_URL
}
#[cfg(windows)]
fn msi_corporate_url() -> &'static str {
    MSI_CORPORATE_URL
}
#[cfg(windows)]
fn exe_global_url() -> &'static str {
    EXE_GLOBAL_URL
}
#[cfg(windows)]
fn exe_corporate_url() -> &'static str {
    EXE_CORPORATE_URL
}
#[cfg(not(windows))]
fn msi_global_url() -> &'static str {
    ""
}
#[cfg(not(windows))]
fn msi_corporate_url() -> &'static str {
    ""
}
#[cfg(not(windows))]
fn exe_global_url() -> &'static str {
    ""
}
#[cfg(not(windows))]
fn exe_corporate_url() -> &'static str {
    ""
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

// ── Windows MSI / EXE installer strategies (v1.0.1+) ───────────────

/// msiexec exit code returned when the install completed successfully but a
/// reboot is required to finalize a file replacement (Windows Installer's
/// Restart Manager couldn't replace a locked file in-place and scheduled a
/// `MoveFileEx`-style delete-on-reboot instead).
#[cfg(windows)]
const MSI_EXIT_REBOOT_REQUIRED: i32 = 3010;

/// Download a file from `url` to `path` over HTTPS. Used by the MSI/EXE
/// strategies to fetch the matching installer to `%TEMP%` before launching it.
/// TLS validation is enforced by `ureq`; the caller then re-fetches the
/// `.sha256` sidecar and runs `verify_checksum` for defense against a
/// corporate-proxy interception or a tampered release asset.
#[cfg(windows)]
fn download_to_file(url: &str, path: &std::path::Path) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(120))
        .build();

    let resp = agent
        .get(url)
        .set("User-Agent", &format!("qork/{VERSION}"))
        .call()
        .map_err(|e| format!("Request failed: {e}"))?;

    let mut file = std::fs::File::create(path)
        .map_err(|e| format!("Failed to create temp file {}: {e}", path.display()))?;
    let mut reader = resp.into_reader();
    std::io::copy(&mut reader, &mut file).map_err(|e| format!("Failed to write temp file: {e}"))?;
    Ok(())
}

/// Fetch the cargo-dist `.sha256` sidecar at `<url>.sha256` and return the
/// file contents.
///
/// Format: `<lowercase-64-char-hex>  *<filename>` per cargo-dist's
/// `dist-manifest.json` generation and the parallel implementation in
/// `.github/workflows/windows-installers.yml`. Tolerant of trailing
/// whitespace / missing asterisk via `parse_sha256_sidecar`.
#[cfg(windows)]
fn fetch_sha256_sidecar(url: &str) -> Result<String, String> {
    let sidecar_url = format!("{url}.sha256");
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let resp = agent
        .get(&sidecar_url)
        .set("User-Agent", &format!("qork/{VERSION}"))
        .call()
        .map_err(|e| format!("Sidecar request failed ({sidecar_url}): {e}"))?;
    resp.into_string()
        .map_err(|e| format!("Failed to read sidecar body: {e}"))
}

/// Extract the 64-char hex from a `.sha256` sidecar line. Returns `None` when
/// the first whitespace-separated token is not exactly 64 hex characters.
#[cfg(windows)]
fn parse_sha256_sidecar(content: &str) -> Option<String> {
    content
        .split_whitespace()
        .next()
        .filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|s| s.to_lowercase())
}

/// Compute the SHA-256 of `path`, returning the lowercase hex.
#[cfg(windows)]
fn compute_sha256(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("Failed to hash: {e}"))?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Fetch the `.sha256` sidecar, compute the SHA-256 of the downloaded
/// installer, refuse to proceed on mismatch.
///
/// Defends against a network MITM that replaces the installer bytes in flight
/// (corporate TLS-interception proxies with a trusted root CA, hostile WiFi,
/// captive portals). The sidecar is fetched in a separate request — an
/// attacker would have to corrupt both the installer and the sidecar in a way
/// that yields a matching hash, which is preimage-hard.
#[cfg(windows)]
fn verify_checksum(installer_path: &std::path::Path, installer_url: &str) -> Result<(), String> {
    println!("  Verifying SHA256 checksum...");
    let sidecar_content = fetch_sha256_sidecar(installer_url)?;
    let expected = parse_sha256_sidecar(&sidecar_content).ok_or_else(|| {
        format!("Malformed .sha256 sidecar from {installer_url}.sha256: {sidecar_content:?}")
    })?;
    let actual = compute_sha256(installer_path)?;
    checksum_verdict(&actual, &expected)
}

/// Compare a computed SHA-256 against the expected sidecar hash, refusing on
/// mismatch. Separated from the network fetch + file read in `verify_checksum`
/// so the load-bearing refusal-on-mismatch is unit-testable on any target.
#[cfg(any(target_os = "windows", test))]
fn checksum_verdict(actual: &str, expected: &str) -> Result<(), String> {
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(format!(
            "SHA256 mismatch — refusing to run installer.\n         Expected: {expected}\n         Got:      {actual}\n         This usually indicates a corrupted download or a network MITM."
        ))
    }
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
/// (both stripped of any prerelease/build-metadata suffix). An empty
/// `installed` never matches (covers the `--version` parse failing).
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

/// After the installer reports success, re-exec `<current_exe> --version` and
/// confirm the on-disk binary has been updated. Catches the case where the
/// installer exits 0 but the file replacement didn't actually take effect
/// (Restart Manager edge cases, MSI re-running the SAME version, etc.).
///
/// Compares the post-install version against `expected` (the `latest` tag from
/// the GitHub releases API). Both are stripped of any prerelease/build-metadata
/// suffix before comparison.
#[cfg(windows)]
fn verify_post_install(expected: &str) -> Result<(), String> {
    let installed = reexec_installed_version()
        .ok_or_else(|| "Failed to run `qork --version` to confirm the install".to_string())?;
    if post_install_version_ok(&installed, expected) {
        Ok(())
    } else {
        Err(format!(
            "Installer exited successfully but `qork --version` still reports v{installed} (expected v{expected}). The installed binary may be locked by another process — close other qork windows / shells and re-run, or reboot to let Windows finish a deferred file replace."
        ))
    }
}

/// Download the matching MSI, verify its SHA256, re-run it via
/// `msiexec /i /passive /norestart`, then re-exec the binary with `--version`
/// to confirm the file replacement actually took effect.
///
/// WiX `MajorUpgrade` in `wix/main.wxs` and `wix-corporate/corporate.wxs`
/// handles the uninstall-old-then-install-new step atomically. Windows
/// Installer's Restart Manager handles the "binary in use" case by renaming
/// the locked file (the running qork process keeps its open file handle to the
/// OLD inode); if RM falls back to delete-on-reboot, msiexec returns 3010 and
/// we surface that without claiming success.
#[cfg(windows)]
fn try_msi_install(url: &str, latest: &str) -> Result<(), StrategyError> {
    let temp_path = std::env::temp_dir().join(format!("qork-update-{latest}.msi"));

    let result = (|| -> Result<(), StrategyError> {
        println!("  Downloading MSI installer...");
        download_to_file(url, &temp_path)
            .map_err(|e| StrategyError::Runtime(format!("Download failed: {e}")))?;

        verify_checksum(&temp_path, url).map_err(StrategyError::Runtime)?;

        println!("  Launching Windows Installer...");
        // /passive shows a progress dialog with no user interaction; /norestart
        // suppresses any reboot prompt (we don't need a reboot for a simple file
        // replace). For the Global perMachine MSI, msiexec triggers UAC before
        // doing anything; for the Corporate perUser MSI, it installs silently
        // into LocalAppData with no elevation prompt.
        let status = Command::new("msiexec")
            .args(["/i", &temp_path.to_string_lossy(), "/passive", "/norestart"])
            .status()
            .map_err(|e| StrategyError::Preflight(format!("Failed to spawn msiexec: {e}")))?;

        let code = status.code().unwrap_or(-1);
        if code == MSI_EXIT_REBOOT_REQUIRED {
            // Install completed but Restart Manager couldn't finalize a file
            // replace in-place. Surface this rather than silently claiming
            // success — `verify_post_install` would fail because the on-disk
            // binary is still old.
            return Err(StrategyError::Runtime(format!(
                "MSI install completed but requires a reboot to finalize (msiexec exit {MSI_EXIT_REBOOT_REQUIRED}). Reboot, then verify with `qork --version`."
            )));
        }
        if !status.success() {
            return Err(StrategyError::Runtime(format!(
                "msiexec exited with code {code} (likely user cancel, UAC denied, or install error)"
            )));
        }

        verify_post_install(latest).map_err(StrategyError::Runtime)?;
        Ok(())
    })();

    // Best-effort: don't leave the downloaded installer behind in %TEMP%, on
    // success or failure. The SHA256 + post-install verify above are the
    // load-bearing checks; this cleanup is pure hygiene and never alters the
    // result. (msiexec has already exited, so the file is no longer in use.)
    let _ = std::fs::remove_file(&temp_path);
    result
}

/// Download the matching Inno Setup EXE installer, verify its SHA256, re-run it
/// with `/SILENT /SUPPRESSMSGBOXES /NORESTART`, then verify the post-install
/// version.
///
/// Inno Setup's AppId-based upgrade detection silently uninstalls the old
/// version before installing the new one. For the Global perMachine EXE,
/// `PrivilegesRequired=admin` in `inno/global.iss` triggers UAC before any UI;
/// the Corporate perUser EXE (`PrivilegesRequired=lowest`) installs without
/// elevation.
#[cfg(windows)]
fn try_exe_install(url: &str, latest: &str) -> Result<(), StrategyError> {
    let temp_path = std::env::temp_dir().join(format!("qork-update-{latest}-setup.exe"));

    let result = (|| -> Result<(), StrategyError> {
        println!("  Downloading EXE installer...");
        download_to_file(url, &temp_path)
            .map_err(|e| StrategyError::Runtime(format!("Download failed: {e}")))?;

        verify_checksum(&temp_path, url).map_err(StrategyError::Runtime)?;

        println!("  Launching Inno Setup installer...");
        // /SILENT shows a progress dialog but no wizard pages; /SUPPRESSMSGBOXES
        // suppresses non-critical message boxes; /NORESTART skips reboot prompts.
        let status = Command::new(&temp_path)
            .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
            .status()
            .map_err(|e| StrategyError::Preflight(format!("Failed to spawn EXE installer: {e}")))?;

        if !status.success() {
            return Err(StrategyError::Runtime(format!(
                "EXE installer exited with code {} (likely user cancel, UAC denied, or install error)",
                status.code().unwrap_or(-1)
            )));
        }

        verify_post_install(latest).map_err(StrategyError::Runtime)?;
        Ok(())
    })();

    // Best-effort cleanup of the downloaded installer (success or failure); the
    // verifies above are the load-bearing checks. The installer process has
    // already exited by here.
    let _ = std::fs::remove_file(&temp_path);
    result
}

#[cfg(not(windows))]
fn try_msi_install(_url: &str, _latest: &str) -> Result<(), StrategyError> {
    Err(StrategyError::Preflight(
        "MSI installer is Windows-only".into(),
    ))
}

#[cfg(not(windows))]
fn try_exe_install(_url: &str, _latest: &str) -> Result<(), StrategyError> {
    Err(StrategyError::Preflight(
        "EXE installer is Windows-only".into(),
    ))
}

// ── Windows install-origin detection (v1.0.1+) ─────────────────────

/// Read the `HKCU\Software\Qork\InstallSource` registry value written by the
/// four first-class installers on install. Authoritative when present. Returns
/// `None` if the key is missing, the value is missing, the value type isn't a
/// string, or the value content doesn't match a known variant.
#[cfg(windows)]
fn read_install_source_marker() -> Option<InstallOrigin> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Software\\Qork").ok()?;
    let value: String = key.get_value("InstallSource").ok()?;
    match value.as_str() {
        "msi-global" => Some(InstallOrigin::MsiGlobal),
        "msi-corporate" => Some(InstallOrigin::MsiCorporate),
        "exe-global" => Some(InstallOrigin::ExeGlobal),
        "exe-corporate" => Some(InstallOrigin::ExeCorporate),
        _ => None,
    }
}

/// Determine how this binary was installed on Windows.
///
/// Strategy:
/// 1. Read the `HKCU\Software\Qork\InstallSource` marker. Authoritative when
///    present. All four first-class installers (Global MSI, Corporate MSI,
///    Global EXE, Corporate EXE) write this marker on install.
/// 2. If no marker, fall back to path-based detection on the running binary's
///    location. Handles the `cargo install` / shell / PowerShell installer
///    path (which doesn't write a marker), and any pre-marker legacy installs.
///
/// The path fallback maps Program Files → `MsiGlobal` and
/// LocalAppData\Programs → `MsiCorporate` (it can't distinguish MSI vs EXE when
/// the marker is missing because both installer formats target the same paths
/// within each edition — that's by design, see README "pick one format per
/// edition"). When the marker IS present, the EXE vs MSI distinction is
/// preserved.
#[cfg(windows)]
pub(crate) fn detect_install_origin() -> InstallOrigin {
    if let Some(origin) = read_install_source_marker() {
        return origin;
    }

    let Ok(exe) = std::env::current_exe() else {
        return InstallOrigin::Unknown;
    };
    classify_install_path(&exe.to_string_lossy())
}

/// Pure-function half of `detect_install_origin()` for unit testing. Lowercased
/// substring match handles drive-letter casing and Windows path
/// case-insensitivity. Order matters: check more-specific paths first.
#[cfg(windows)]
fn classify_install_path(exe_path: &str) -> InstallOrigin {
    let lower = exe_path.to_lowercase();
    if lower.contains("\\program files\\qork\\") {
        InstallOrigin::MsiGlobal
    } else if lower.contains("\\appdata\\local\\programs\\qork\\") {
        InstallOrigin::MsiCorporate
    } else if lower.contains("\\.cargo\\bin\\") {
        InstallOrigin::CargoOrInstaller
    } else {
        InstallOrigin::Unknown
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
    fn is_newer_major_versions() {
        assert!(is_newer("3.8.0", "4.0.0"));
        assert!(!is_newer("4.0.0", "3.99.99"));
    }

    #[test]
    fn strip_prerelease_metadata_cases() {
        assert_eq!(strip_prerelease_metadata("1.2.3-rc.1"), "1.2.3");
        assert_eq!(strip_prerelease_metadata("1.2.3+sha.abc"), "1.2.3");
        assert_eq!(strip_prerelease_metadata("1.2.3"), "1.2.3");
        assert_eq!(strip_prerelease_metadata("1.0"), "1.0");
        assert_eq!(strip_prerelease_metadata(""), "");
    }

    #[test]
    fn is_newer_handles_prerelease_and_metadata() {
        assert!(is_newer("1.2.0", "1.2.1-rc.1"));
        assert!(is_newer("1.2.1-rc.1", "1.2.1"));
        assert!(!is_newer("1.2.1", "1.2.1+build.7"));
    }

    #[test]
    fn is_newer_treats_two_prereleases_of_same_triple_as_equal() {
        // GitHub /releases/latest filters prereleases out so this is
        // theoretical; the conservative "equal -> not newer" verdict means
        // qork reports "already on latest".
        assert!(!is_newer("1.2.1-rc.1", "1.2.1-rc.2"));
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
    fn unix_without_cargo_prunes_cargo() {
        assert_eq!(
            order_strategies(false, TargetOs::Unix),
            vec![UpdateStrategy::InstallerCurl, UpdateStrategy::InstallerWget]
        );
    }

    #[test]
    fn windows_with_cargo_orders_cargo_first() {
        assert_eq!(
            order_strategies(true, TargetOs::Windows),
            vec![
                UpdateStrategy::Cargo,
                UpdateStrategy::InstallerPowerShell,
                UpdateStrategy::InstallerPwsh
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

    #[test]
    fn strategy_json_ids_are_stable() {
        // These IDs are part of the public JSON contract; renaming them is a
        // schema break.
        assert_eq!(UpdateStrategy::Cargo.json_id(), "cargo");
        assert_eq!(UpdateStrategy::MsiGlobal.json_id(), "msi_global");
        assert_eq!(UpdateStrategy::MsiCorporate.json_id(), "msi_corporate");
        assert_eq!(UpdateStrategy::ExeGlobal.json_id(), "exe_global");
        assert_eq!(UpdateStrategy::ExeCorporate.json_id(), "exe_corporate");
    }

    #[test]
    fn strategy_labels_are_unique() {
        // Labels feed the text-mode "Updating via X..." line; each must be
        // distinct so users can tell which installer is being downloaded.
        let labels = [
            UpdateStrategy::Cargo.label(),
            UpdateStrategy::InstallerCurl.label(),
            UpdateStrategy::InstallerWget.label(),
            UpdateStrategy::InstallerPowerShell.label(),
            UpdateStrategy::InstallerPwsh.label(),
            UpdateStrategy::MsiGlobal.label(),
            UpdateStrategy::MsiCorporate.label(),
            UpdateStrategy::ExeGlobal.label(),
            UpdateStrategy::ExeCorporate.label(),
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            labels.len(),
            "all strategy labels must be unique"
        );
    }

    #[test]
    fn checksum_verdict_accepts_match_and_refuses_mismatch() {
        let hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        // Exact match passes.
        assert!(checksum_verdict(hash, hash).is_ok());
        // Case-insensitive match passes (sidecars may be upper/lower case).
        assert!(checksum_verdict(&hash.to_uppercase(), hash).is_ok());
        // A mismatch is REFUSED — this is the load-bearing MITM/corruption
        // guard, so it must never silently pass.
        let err = checksum_verdict("deadbeef", hash).unwrap_err();
        assert!(err.contains("SHA256 mismatch"), "err: {err}");
    }

    #[cfg(windows)]
    #[test]
    fn install_origin_classify_program_files_is_msi_global() {
        assert_eq!(
            classify_install_path(r"C:\Program Files\qork\bin\qork.exe"),
            InstallOrigin::MsiGlobal,
        );
        // Case-insensitive: drive letter or "Program Files" capitalization
        // shouldn't change the verdict.
        assert_eq!(
            classify_install_path(r"c:\PROGRAM FILES\qork\BIN\qork.exe"),
            InstallOrigin::MsiGlobal,
        );
    }

    #[cfg(windows)]
    #[test]
    fn install_origin_classify_localappdata_is_msi_corporate() {
        assert_eq!(
            classify_install_path(r"C:\Users\alice\AppData\Local\Programs\qork\bin\qork.exe"),
            InstallOrigin::MsiCorporate,
        );
    }

    #[cfg(windows)]
    #[test]
    fn install_origin_classify_cargo_bin_is_cargo_or_installer() {
        assert_eq!(
            classify_install_path(r"C:\Users\alice\.cargo\bin\qork.exe"),
            InstallOrigin::CargoOrInstaller,
        );
    }

    #[cfg(windows)]
    #[test]
    fn install_origin_classify_random_path_is_unknown() {
        assert_eq!(
            classify_install_path(r"D:\portable\qork\qork.exe"),
            InstallOrigin::Unknown,
        );
        assert_eq!(
            classify_install_path(r"C:\Users\alice\Downloads\qork.exe"),
            InstallOrigin::Unknown,
        );
    }

    #[cfg(windows)]
    #[test]
    fn install_origin_json_ids_are_kebab_case() {
        // The JSON id matches the literal registry marker value written by the
        // installers. Keep these in lockstep with wix/main.wxs,
        // wix-corporate/corporate.wxs, inno/global.iss, inno/corporate.iss.
        assert_eq!(InstallOrigin::MsiGlobal.json_id(), "msi-global");
        assert_eq!(InstallOrigin::MsiCorporate.json_id(), "msi-corporate");
        assert_eq!(InstallOrigin::ExeGlobal.json_id(), "exe-global");
        assert_eq!(InstallOrigin::ExeCorporate.json_id(), "exe-corporate");
        assert_eq!(
            InstallOrigin::CargoOrInstaller.json_id(),
            "cargo-or-installer"
        );
        assert_eq!(InstallOrigin::Unknown.json_id(), "unknown");
    }

    #[cfg(windows)]
    #[test]
    fn parse_sha256_sidecar_accepts_cargo_dist_format() {
        // cargo-dist publishes lines like:
        //   "<hex>  *<filename>"  (two spaces, asterisk-prefixed name)
        let line = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789  *qork-x86_64-pc-windows-msvc.msi";
        assert_eq!(
            parse_sha256_sidecar(line).as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"),
        );
    }

    #[cfg(windows)]
    #[test]
    fn parse_sha256_sidecar_accepts_no_asterisk_variant() {
        // Some sha256sum invocations omit the asterisk binary-mode marker.
        let line = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef qork.msi";
        assert_eq!(
            parse_sha256_sidecar(line).as_deref(),
            Some("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"),
        );
    }

    #[cfg(windows)]
    #[test]
    fn parse_sha256_sidecar_normalizes_to_lowercase() {
        let line = "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789  *foo.msi";
        assert_eq!(
            parse_sha256_sidecar(line).as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"),
        );
    }

    #[cfg(windows)]
    #[test]
    fn parse_sha256_sidecar_rejects_wrong_length() {
        assert_eq!(parse_sha256_sidecar("abcdef  *foo.msi"), None);
        let too_long = format!("{}0  *foo.msi", "a".repeat(64));
        assert_eq!(parse_sha256_sidecar(&too_long), None);
        assert_eq!(parse_sha256_sidecar(""), None);
    }

    #[cfg(windows)]
    #[test]
    fn parse_sha256_sidecar_rejects_non_hex_chars() {
        let bad = format!("{}  *foo.msi", "g".repeat(64));
        assert_eq!(parse_sha256_sidecar(&bad), None);
    }

    #[test]
    fn self_delete_command_quotes_path_and_waits() {
        let cmd = self_delete_command(r"C:\Users\alice\.cargo\bin\qork.exe");
        // Must quote the path (spaces / backslashes) and delete it forcibly.
        assert!(
            cmd.contains(r#"del /f /q "C:\Users\alice\.cargo\bin\qork.exe""#),
            "cmd: {cmd}"
        );
        // Must wait first so the running qork can exit and release the image.
        assert!(cmd.contains("ping 127.0.0.1 -n 3"), "cmd: {cmd}");
        // The ping output is suppressed so the detached helper is silent.
        assert!(cmd.contains(">nul"), "cmd: {cmd}");
    }

    #[test]
    fn self_delete_command_handles_path_with_spaces() {
        let cmd = self_delete_command(r"C:\Program Files\qork\bin\qork.exe");
        assert!(
            cmd.contains(r#"del /f /q "C:\Program Files\qork\bin\qork.exe""#),
            "cmd: {cmd}"
        );
    }

    #[test]
    fn is_qork_display_name_matches_exact_product_names_only() {
        assert!(is_qork_display_name("qork"));
        assert!(is_qork_display_name("qork (Corporate Edition)"));
        // Case-insensitive + tolerant of surrounding whitespace.
        assert!(is_qork_display_name("QORK"));
        assert!(is_qork_display_name("  qork (corporate edition)  "));
        // Must NOT match unrelated products that merely contain "qork".
        assert!(!is_qork_display_name("qorkscrew"));
        assert!(!is_qork_display_name("My qork tool"));
        assert!(!is_qork_display_name("workqork"));
        assert!(!is_qork_display_name(""));
    }

    #[test]
    fn msi_uninstall_command_forces_x_mode() {
        // Real-world UninstallString uses /I (repair mode) — must become /X.
        assert_eq!(
            msi_uninstall_command("MsiExec.exe /I{1C44B9CA-20FD-4AED-A8AE-1D169AE14203}"),
            "MsiExec.exe /X{1C44B9CA-20FD-4AED-A8AE-1D169AE14203}"
        );
        // Already /X — GUID preserved, normalized form.
        assert_eq!(
            msi_uninstall_command("MsiExec.exe /X{AAAA-BBBB}"),
            "MsiExec.exe /X{AAAA-BBBB}"
        );
        // Lowercase + spaced form.
        assert_eq!(
            msi_uninstall_command("msiexec.exe /x {CCCC-DDDD}"),
            "MsiExec.exe /X{CCCC-DDDD}"
        );
        // No GUID → left untouched.
        assert_eq!(msi_uninstall_command("whatever.exe"), "whatever.exe");
    }

    #[test]
    fn split_command_line_handles_quoted_and_bare_programs() {
        // Inno's UninstallString: a quoted path to unins000.exe + a flag.
        let (p, a) = split_command_line(r#""C:\Program Files\qork\unins000.exe" /SILENT"#);
        assert_eq!(p, r"C:\Program Files\qork\unins000.exe");
        assert_eq!(a, vec!["/SILENT"]);

        // MSI form: bare program, GUID arg.
        let (p, a) = split_command_line("MsiExec.exe /X{ABCD-1234}");
        assert_eq!(p, "MsiExec.exe");
        assert_eq!(a, vec!["/X{ABCD-1234}"]);

        // Program with no args.
        let (p, a) = split_command_line("solo.exe");
        assert_eq!(p, "solo.exe");
        assert!(a.is_empty());
    }

    #[test]
    fn cargo_dist_receipt_paths_named_correctly() {
        // The receipt filename is the cargo-dist convention `<app>-receipt.json`.
        // We don't assert the directory (it depends on env vars), only that any
        // candidate path ends with the expected filename so a typo can't slip in.
        for path in cargo_dist_receipt_paths() {
            assert_eq!(
                path.file_name().and_then(|n| n.to_str()),
                Some("qork-receipt.json"),
                "unexpected receipt path: {}",
                path.display()
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn compute_sha256_matches_known_value() {
        // Empty file -> known SHA256 of the empty input.
        let dir = std::env::temp_dir().join(format!("qork-update-tests-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("empty.bin");
        std::fs::write(&path, b"").unwrap();
        let hash = compute_sha256(&path).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
