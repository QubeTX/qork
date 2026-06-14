//! Pre-shorten URL sanity check.
//!
//! The goal is narrow and specific: don't shorten something that was never a
//! real link — a random word or sentence pasted by mistake, or a URL that was
//! invalid from the start. It is deliberately NOT a strict uptime gate: auth
//! walls (401/403), 5xx hiccups, slow sites, and transient connection blips
//! all still shorten. Two layers, both skipped by `--no-check`:
//!
//!   1. **structural** (offline, instant): the input must parse as an
//!      http/https URL whose host looks like a real domain (has a dot) or an IP.
//!      A bare word like `help` or `asdf` becomes `https://help` → no dot →
//!      rejected here, with no network call.
//!   2. **ping** (online, a single HEAD request): refuse on a live 404/410, or
//!      if the host doesn't resolve (DNS failure — "invalid from the
//!      beginning"). Every other outcome proceeds.

use std::net::IpAddr;
use std::time::Duration;

use crate::VERSION;

/// The verdict for a candidate URL.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Looks like a real link — go ahead and shorten.
    Ok,
    /// Doesn't look like a URL at all (offline check). Carries a user message.
    NotUrl(String),
    /// The server responded that the page is gone (404 / 410 Gone).
    Dead(u16),
    /// The host doesn't resolve — the link was never valid.
    NoSuchHost,
}

/// Prepend `https://` when the input has no scheme (mirrors the API's behavior
/// so the structural check + ping see what will actually be shortened).
pub fn normalize_url(input: &str) -> String {
    let s = input.trim();
    if let Some(pos) = s.find("://") {
        // A real scheme is a non-empty run of ASCII letters before "://".
        if pos > 0 && s[..pos].chars().all(|c| c.is_ascii_alphabetic()) {
            return s.to_string();
        }
    }
    format!("https://{s}")
}

/// A domain has a dot (`example.com`); IPv4 has dots; IPv6 host strings carry a
/// colon. A bare single label (`help`, `asdf`) has none → not a real website.
fn host_looks_real(host: &str) -> bool {
    host.contains('.') || host.contains(':') || host.parse::<IpAddr>().is_ok()
}

/// Offline structural check on an already-normalized URL.
pub fn structural_verdict(normalized: &str) -> Verdict {
    let parsed = match url::Url::parse(normalized) {
        Ok(u) => u,
        Err(_) => return Verdict::NotUrl("that doesn't look like a URL".into()),
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Verdict::NotUrl(format!(
            "only http and https links can be shortened (got scheme '{}')",
            parsed.scheme()
        ));
    }
    match parsed.host_str() {
        Some(host) if host_looks_real(host) => Verdict::Ok,
        Some(host) => Verdict::NotUrl(format!(
            "'{host}' doesn't look like a website — did you mean to paste a full URL?"
        )),
        None => Verdict::NotUrl("that doesn't look like a URL (it has no host)".into()),
    }
}

/// Online liveness ping — a single HEAD request (following redirects). Only
/// ever blocks on a live 404/410 or a DNS-resolution failure; everything else
/// (auth, 5xx, timeouts, connection refused, TLS quirks) returns `Ok`.
pub fn ping(normalized: &str) -> Verdict {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .redirects(8)
        .build();

    // A browser-ish User-Agent: this is just a reachability probe, and some
    // servers 404/403 unknown agents. The real shorten POST still identifies
    // itself as qork/<version> for source attribution.
    let ua = format!("Mozilla/5.0 (compatible; qork/{VERSION}; +https://qork.me)");

    match agent
        .head(normalized)
        .set("User-Agent", &ua)
        .set("Accept", "*/*")
        .call()
    {
        Ok(_) => Verdict::Ok,
        Err(ureq::Error::Status(code, _)) => {
            if code == 404 || code == 410 {
                Verdict::Dead(code)
            } else {
                // 401/403 (auth), 405, 429, 5xx, … — the link exists.
                Verdict::Ok
            }
        }
        Err(ureq::Error::Transport(t)) => {
            if t.kind() == ureq::ErrorKind::Dns {
                Verdict::NoSuchHost
            } else {
                // Connection refused / timeout / TLS / etc. — a transient blip,
                // not proof the link is bad. Shorten anyway.
                Verdict::Ok
            }
        }
    }
}

/// Full pre-shorten check. When `enabled` is false (`--no-check`), skips
/// everything and returns `Ok`. Otherwise runs the offline structural check,
/// then the online ping only if the structure is sound.
pub fn check(input: &str, enabled: bool) -> Verdict {
    if !enabled {
        return Verdict::Ok;
    }
    let normalized = normalize_url(input);
    match structural_verdict(&normalized) {
        Verdict::Ok => ping(&normalized),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_adds_https_when_missing() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(normalize_url("https://x.com"), "https://x.com");
        assert_eq!(normalize_url("http://x.com/a"), "http://x.com/a");
        assert_eq!(normalize_url("  example.com  "), "https://example.com");
    }

    #[test]
    fn structural_accepts_real_domains_and_ips() {
        assert_eq!(structural_verdict("https://example.com"), Verdict::Ok);
        assert_eq!(
            structural_verdict("https://sub.example.co.uk/a?b=1"),
            Verdict::Ok
        );
        assert_eq!(structural_verdict("http://93.184.216.34"), Verdict::Ok);
    }

    #[test]
    fn structural_rejects_bare_words() {
        // The exact bug: `qork help` / a pasted word becomes https://<word>.
        assert!(matches!(
            structural_verdict("https://help"),
            Verdict::NotUrl(_)
        ));
        assert!(matches!(
            structural_verdict("https://asdf"),
            Verdict::NotUrl(_)
        ));
    }

    #[test]
    fn structural_rejects_non_http_schemes() {
        assert!(matches!(
            structural_verdict("ftp://files.example.com"),
            Verdict::NotUrl(_)
        ));
    }

    #[test]
    fn disabled_check_always_ok() {
        // --no-check forces everything through, even a bare word.
        assert_eq!(check("help", false), Verdict::Ok);
        assert_eq!(check("literally anything", false), Verdict::Ok);
    }
}
