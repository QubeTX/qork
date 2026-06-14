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

`qork` ships **prebuilt binaries** — no Rust toolchain required. The installer drops a single
`qork` binary into `~/.cargo/bin` (`%USERPROFILE%\.cargo\bin` on Windows) and puts it on your PATH.

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

### Manual download

Grab the archive for your platform from the
[latest release](https://github.com/QubeTX/qork/releases/latest), unpack it, and put the
`qork` binary somewhere on your PATH.

| Platform | Architecture | Asset |
|---|---|---|
| macOS | Apple Silicon (M1+) | `qork-aarch64-apple-darwin.tar.xz` |
| macOS | Intel | `qork-x86_64-apple-darwin.tar.xz` |
| Linux | x86_64 (glibc) | `qork-x86_64-unknown-linux-gnu.tar.xz` |
| Linux | ARM64 | `qork-aarch64-unknown-linux-gnu.tar.xz` |
| Linux | x86_64 (musl / Alpine) | `qork-x86_64-unknown-linux-musl.tar.xz` |
| Windows | x86_64 | `qork-x86_64-pc-windows-msvc.msi` (or `.zip`) |

Each asset has a matching `.sha256` sidecar for verification.

**Native installers** ship with every release too — Windows `.msi`/`.exe` (Global per-machine +
Corporate per-user), macOS `.pkg` (Apple Silicon + Intel), and Linux `.deb`/`.rpm` (x86_64 + ARM64).
The full matrix with one-click downloads is at **<https://qork.me/install>**. On macOS and Linux the
one-liner above is the recommended path; on Windows the MSI/EXE installer is recommended.

### Updating & uninstalling

```sh
qork update      # check GitHub Releases and update in place
qork uninstall   # remove the installed binary
```

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

### Options

| Flag | Description |
|---|---|
| `-a`, `--alias <ALIAS>` | Request a custom short code (3–50 chars, letters/numbers/hyphens) |
| `--json` | Print the raw JSON response instead of just the URL (great for scripts/agents) |
| `--no-color` | Disable colored output |
| `--api-base <URL>` | Call a different API base (default `https://qork.me`); also `QORK_API_BASE` |
| `-h`, `--help` | Print help |
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

Releases are automated by [cargo-dist](https://github.com/axodotdev/cargo-dist). Bump the
`version` in `Cargo.toml`, commit, then push a matching tag:

```sh
git tag v1.0.0
git push origin v1.0.0
```

The `release` workflow builds the per-target binaries and installers and publishes a GitHub
Release; the `crates-publish` workflow publishes to crates.io after CI passes on `main`.

---

## License

[PolyForm Noncommercial License 1.0.0](LICENSE) — © 2026 Emmett S.
