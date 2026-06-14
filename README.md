# qork

**Shorten URLs from your terminal.** `qork` is a tiny, fast, cross-platform CLI for
[qork.me](https://qork.me) — paste a URL, get a short link back. Written in Rust; ships as a
single prebuilt binary for macOS, Linux, and Windows (Intel + ARM). No runtime, no config.

```console
$ qork https://example.com/some/very/long/path
https://qork.me/ka9m
```

> A [QubeTX](https://qubetx.com) property. Sibling of `tr300` (qube-machine-report) and
> `wb300` (qube-workbranch-view).

---

## Installation

`qork` ships **prebuilt binaries** — no Rust toolchain required. The one-liner / `cargo install`
path drops a single `qork` binary on your PATH (in `~/.cargo/bin`, or `%USERPROFILE%\.cargo\bin`
on Windows); the native installers below place it in their own per-OS location instead. On macOS
and Linux the one-liner is the recommended path; on Windows the MSI/EXE installer is recommended.

The full matrix with one-click downloads (and a live "latest version" badge) is at
**<https://qork.me/install>**.

### macOS / Linux

```sh
curl -LsSf https://qork.me/install.sh | sh
```

### Windows (PowerShell)

```powershell
irm https://qork.me/install.ps1 | iex
```

### Cargo

If you have a Rust toolchain:

```sh
cargo install qork
```

### Native installers

Every release ships native OS installers alongside the archives, each with a matching `.sha256`
sidecar, under `github.com/QubeTX/qork/releases/latest/download/<asset>`:

| Platform | Edition / arch | Asset | Installs to |
|---|---|---|---|
| Windows | Global MSI (per-machine, admin) | `qork-x86_64-pc-windows-msvc.msi` | `C:\Program Files\qork\bin\` |
| Windows | Corporate MSI (per-user, no admin) | `qork-x86_64-pc-windows-msvc-corporate.msi` | `%LocalAppData%\Programs\qork\bin\` |
| Windows | Global EXE (Inno, per-machine) | `qork-x86_64-pc-windows-msvc-setup.exe` | `C:\Program Files\qork\bin\` |
| Windows | Corporate EXE (Inno, per-user) | `qork-x86_64-pc-windows-msvc-corporate-setup.exe` | `%LocalAppData%\Programs\qork\bin\` |
| macOS | Apple Silicon (`.pkg`) | `qork-aarch64-apple-darwin.pkg` | `/usr/local/bin/qork` |
| macOS | Intel (`.pkg`) | `qork-x86_64-apple-darwin.pkg` | `/usr/local/bin/qork` |
| Linux | x86_64 `.deb` / `.rpm` | `qork-x86_64-unknown-linux-gnu.{deb,rpm}` | `/usr/bin/qork` |
| Linux | ARM64 `.deb` / `.rpm` | `qork-aarch64-unknown-linux-gnu.{deb,rpm}` | `/usr/bin/qork` |

- **Windows:** pick **one** format per edition (don't install both the MSI and the EXE — they'd
  register two Add/Remove-Programs entries). Global needs admin (UAC); Corporate installs per-user
  with no elevation. All four write an `HKCU\Software\Qork\InstallSource` marker so `qork update`
  re-runs the matching installer for an in-place upgrade.
- **macOS:** the `.pkg` is **unsigned** (no Apple Developer ID), so the first launch needs a
  right-click → Open (or `xattr -d com.apple.quarantine`). The `curl | sh` one-liner is the
  recommended macOS path; the `.pkg` is a convenience.
- **Linux:** `.deb` and `.rpm` install `man qork` too (`/usr/share/man/man1/qork.1`).

### Manual download (archives)

Prefer the one-liner or a native installer above. If you want the raw binary, grab the archive for
your platform from the [latest release](https://github.com/QubeTX/qork/releases/latest), unpack it,
and put the `qork` binary somewhere on your PATH. Each archive also has a `.sha256` sidecar.

| Platform | Architecture | Asset |
|---|---|---|
| macOS | Apple Silicon (M1+) | `qork-aarch64-apple-darwin.tar.xz` |
| macOS | Intel | `qork-x86_64-apple-darwin.tar.xz` |
| Linux | x86_64 (glibc) | `qork-x86_64-unknown-linux-gnu.tar.xz` |
| Linux | ARM64 | `qork-aarch64-unknown-linux-gnu.tar.xz` |
| Linux | x86_64 (musl / Alpine) | `qork-x86_64-unknown-linux-musl.tar.xz` |
| Windows | x86_64 | `qork-x86_64-pc-windows-msvc.zip` |

### Updating & uninstalling

```sh
qork update            # update in place — re-runs the matching installer (MSI/EXE/cargo/shell)
qork uninstall         # fully remove qork from this system (every platform)
qork uninstall --yes   # …without the confirmation prompt (for scripts/CI)
```

`qork update` is aware of how qork was installed and re-runs the matching installer: on Windows it
reads the `HKCU\Software\Qork\InstallSource` marker to pick the matching Global/Corporate MSI or
Inno EXE (downloading it, SHA256-verifying it against the release `.sha256` sidecar, then
re-checking `qork --version`); a Windows `cargo` / PowerShell-installer install falls through to a
`cargo install qork --force` → `irm|iex` chain. **On macOS/Linux, qork update prefers the prebuilt
`curl|sh` (then `wget`) installer and only falls back to `cargo install qork --force` as a last
resort** — so updating needs no Rust toolchain and doesn't recompile from source (a cargo-less
machine updates fine; a machine with cargo still gets the fast prebuilt path first).

`qork uninstall` fully removes qork on every platform — including Windows. It's origin-aware:

- **Windows MSI** (Global or Corporate) → runs the recorded Add/Remove-Programs uninstaller via
  `msiexec /x … /passive /norestart` (Global prompts for UAC; Corporate is silent).
- **Windows Inno EXE** → runs the recorded uninstaller `/SILENT /SUPPRESSMSGBOXES /NORESTART`.
- **Windows cargo / shell / PowerShell** → a running `.exe` can't delete itself, so qork schedules a
  detached helper that waits for the process to exit and then deletes the binary.
- **macOS / Linux** → unlinks the binary directly (a running Unix binary can be unlinked).

In every case it also removes the cargo-dist install receipt and the `HKCU\Software\Qork` marker,
and notes the PATH entry the installer added (qork doesn't edit shell profiles). `--yes` (or `-y`)
skips the confirmation prompt and is **required** when stdin isn't a terminal (scripts/CI).

---

## Usage

Shorten a normal URL by pasting it straight in:

```sh
qork https://example.com/some/very/long/path
```

If the URL contains spaces or shell-special characters (`&`, `?`, `#`, …), wrap it in quotes:

```sh
qork "https://example.com/search?q=a b&page=2"
```

`qork` prints **only** the short URL to stdout, so it pipes and substitutes cleanly:

```sh
qork https://example.com | pbcopy          # macOS — copy to clipboard
LINK=$(qork https://example.com)           # capture in a variable
```

### Commands & the safety check

`qork help`, `qork update`, and `qork uninstall` are recognized as commands (whole-word,
case-insensitive) — never treated as URLs. The bare words `help` / `update` / `uninstall` work the
same as the `--help` / `--update` / `--uninstall` flags. Everything else is shortened. `qork help`
prints the full help; `man qork` works too on `.deb`/`.rpm`/`.pkg` installs.

| Command | What it does |
|---|---|
| `qork <url>` | Shorten a URL and print the short link |
| `qork help` | Print the full CLI help (same as `--help`; also `man qork`) |
| `qork update` | Self-update in place (install-method-aware — see above) |
| `qork uninstall [--yes]` | Fully remove qork from this system (see above) |

Before shortening, qork checks the link is real, in two layers (both skipped by `--no-check`):

1. **Offline / structural** — the input must parse as an `http`/`https` URL whose host looks like a
   real domain (has a dot) or an IP. A bare word like `asdf` becomes `https://asdf` → no dot →
   rejected with no network call. (This is also why a mistyped command never gets shortened.)
2. **Online ping** — a single HEAD request. qork refuses only on a live **404/410** or a host that
   **doesn't resolve** (DNS failure — the link was never valid). Auth walls (401/403), 405/429,
   5xx, timeouts, connection-refused, and TLS quirks all still shorten — the check is a typo/dead-link
   guard, not a strict uptime gate.

### Options

| Flag | Description |
|---|---|
| `-a`, `--alias <ALIAS>` | Request a custom short code (3–50 chars, letters/numbers/hyphens) |
| `--json` | Print the raw JSON response instead of just the URL (great for scripts/agents) |
| `--no-check` | Skip the pre-shorten check (shorten without verifying the link is live) |
| `--no-color` | Disable colored output |
| `--api-base <URL>` | Call a different API base (default `https://qork.me`); also `QORK_API_BASE` |
| `-y`, `--yes` | Skip the `uninstall` confirmation prompt (required for non-interactive use) |
| `-h`, `--help` | Print help (`qork help` and `man qork` also work) |
| `-V`, `--version` | Print version |

### Examples

```sh
# Custom alias
qork https://example.com/launch --alias launch

# Machine-readable output (agents / scripts)
qork --json https://example.com
# {"id":"…","shortCode":"ka9m","shortUrl":"qork.me/ka9m","href":"https://qork.me/ka9m",
#  "longUrl":"https://example.com","isNew":true,"domain":"example.com","createdAt":"…"}

# Point at a local dev server
QORK_API_BASE=http://localhost:3000 qork https://example.com
```

### Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `2` | Error (invalid URL, alias taken, network/API failure) — message on stderr |

---

## API reference

`qork` is a thin client over the public qork.me shorten API. You can call it directly from
`curl`, another program, or an agent.

### `POST /api/shorten`

Request body (`application/json`):

```json
{ "url": "https://example.com/long/path", "customAlias": "my-link", "source": "cli" }
```

- `url` (required) — the URL to shorten.
- `customAlias` (optional) — a custom short code.
- `source` (optional) — `web` | `cli` | `api`, recorded for analytics. `qork` sends `cli`.

### `GET /api/shorten?url=<url-encoded>[&alias=<alias>]`

A convenience form for quick `curl` / agent use — same JSON response as POST:

```sh
curl "https://qork.me/api/shorten?url=https%3A%2F%2Fexample.com"
```

### Response

```json
{
  "id": "…",
  "shortCode": "ka9m",
  "shortUrl": "qork.me/ka9m",
  "href": "https://qork.me/ka9m",
  "longUrl": "https://example.com/long/path",
  "isNew": true,
  "domain": "example.com",
  "createdAt": "2026-06-14T09:57:58Z"
}
```

`href` is the fully-qualified short link; `shortUrl` is the same without the scheme.
`isNew` is `false` when the URL was already shortened and the existing link was returned.
Errors return `{ "error": "…" }` with an appropriate 4xx/5xx status.

For the full agent guide, see <https://qork.me/llms.txt>.

---

## Development

Requires Rust 1.95+.

```sh
cargo build            # debug build
cargo test             # unit + integration tests (no network)
cargo clippy --all-targets -- -D warnings
cargo fmt --all

# Manual live check against the API:
cargo run -- https://example.com
```

### Releasing

To cut a release, bump the `version` in `Cargo.toml`, commit, and push to `main` (CI →
`crates-publish.yml` publishes to crates.io once CI is green). Then push a matching tag:

```sh
git tag v1.1.1
git push origin v1.1.1
```

The tag push fans out to **three** workflows that all upload to the same GitHub Release:

| Workflow | Builds |
|---|---|
| `release.yml` (cargo-dist) | per-target `.tar.xz`/`.zip` archives, the shell + PowerShell installers, and the **Global MSI**; creates the GitHub Release |
| `windows-installers.yml` | the **Corporate MSI** (WiX) + the **Global & Corporate Inno EXEs** |
| `unix-installers.yml` | the macOS **`.pkg`** (×2 arch, `pkgbuild`) + Linux **`.deb`/`.rpm`** (×2 arch, `nfpm`) |

`windows-installers.yml` and `unix-installers.yml` are hand-authored (not generated by cargo-dist);
both poll until `release.yml` has created the Release, then `gh release upload --clobber` their
add-on assets onto it (idempotent, no race). Two more workflows round out CI: `ci.yml`
(fmt + clippy `-D warnings` + tests on Linux/macOS/Windows + a release build) and
`crates-publish.yml` (publishes to crates.io after CI passes on `main`; skips if the version is
already published).

---

## License

[PolyForm Noncommercial License 1.0.0](LICENSE) — © 2026 Emmett S.
