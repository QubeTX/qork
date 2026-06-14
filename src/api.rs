//! The qork.me shorten API client.
//!
//! One blocking round trip: POST `{api_base}/api/shorten` with
//! `{"url", "customAlias"?, "source": "cli"}` and read back the JSON envelope
//! `{ shortCode, shortUrl, href, longUrl, isNew, ... }`. The `source` field
//! lets the qork.me admin console tell CLI shortens apart from website ones;
//! the `User-Agent: qork/<version>` header is a second, header-level signal.

use std::time::Duration;

use serde_json::Value;

use crate::config::Config;
use crate::error::{AppError, Result};
use crate::VERSION;

/// A created (or pre-existing) short link, plus the raw JSON for `--json`.
#[derive(Debug, Clone)]
pub struct ShortLink {
    pub short_code: String,
    /// Scheme-less short URL as the API returns it, e.g. `qork.me/ka9m`.
    pub short_url: String,
    /// Fully-qualified URL (`https://qork.me/ka9m`) when the API provides it.
    pub href: Option<String>,
    /// False when the URL was already shortened and the existing link came back.
    pub is_new: bool,
    /// The verbatim JSON envelope, preserved for `--json` fidelity.
    pub raw: Value,
}

impl ShortLink {
    fn from_value(raw: Value) -> Result<Self> {
        let short_code = raw
            .get("shortCode")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let short_url = raw
            .get("shortUrl")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let href = raw.get("href").and_then(Value::as_str).map(str::to_string);
        let is_new = raw.get("isNew").and_then(Value::as_bool).unwrap_or(false);

        if short_code.is_empty() && short_url.is_empty() && href.is_none() {
            return Err(AppError::api(
                "the server returned no short link in its response",
            ));
        }

        Ok(Self {
            short_code,
            short_url,
            href,
            is_new,
            raw,
        })
    }

    /// The URL to print: prefer the fully-qualified `href`; otherwise prefix
    /// `https://` onto the scheme-less `shortUrl` (e.g. `qork.me/ka9m`).
    pub fn display_url(&self) -> String {
        if let Some(href) = self.href.as_deref() {
            if !href.is_empty() {
                return href.to_string();
            }
        }
        let s = self.short_url.trim();
        if s.starts_with("http://") || s.starts_with("https://") {
            s.to_string()
        } else {
            format!("https://{s}")
        }
    }
}

/// Shorten `url` against `{api_base}/api/shorten`. `alias`, when non-empty,
/// requests a custom short code. Returns the created (or existing) link.
pub fn shorten(url: &str, alias: Option<&str>, config: &Config) -> Result<ShortLink> {
    let url = url.trim();
    if url.is_empty() {
        return Err(AppError::invalid_url("a URL is required"));
    }
    // Fast, offline guard for the single most common mistake — an unquoted URL
    // with a space. The server is still the source of truth for real validation
    // (scheme, host, reserved words); this just fails obvious cases instantly.
    if url.chars().any(char::is_whitespace) {
        return Err(AppError::invalid_url(
            "the URL contains spaces — wrap it in quotes, e.g. qork \"https://example.com/a b\"",
        ));
    }

    let endpoint = format!("{}/api/shorten", config.api_base);

    let mut body = serde_json::json!({ "url": url, "source": "cli" });
    if let Some(alias) = alias {
        let alias = alias.trim();
        if !alias.is_empty() {
            body["customAlias"] = Value::String(alias.to_string());
        }
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();

    match agent
        .post(&endpoint)
        .set("User-Agent", &format!("qork/{VERSION}"))
        .set("Accept", "application/json")
        .send_json(body)
    {
        Ok(resp) => {
            let raw: Value = resp
                .into_json()
                .map_err(|e| AppError::api(format!("could not parse the server response: {e}")))?;
            ShortLink::from_value(raw)
        }
        // 4xx/5xx — surface the server's own `error` message when present.
        Err(ureq::Error::Status(code, resp)) => {
            let message = resp
                .into_json::<Value>()
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
                .unwrap_or_else(|| format!("the server returned HTTP {code}"));
            Err(AppError::api(message))
        }
        Err(ureq::Error::Transport(t)) => Err(AppError::api(format!(
            "could not reach {} ({t})",
            config.api_base
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_url_prefers_href() {
        let link = ShortLink::from_value(serde_json::json!({
            "shortCode": "ka9m",
            "shortUrl": "qork.me/ka9m",
            "href": "https://qork.me/ka9m",
            "isNew": true
        }))
        .unwrap();
        assert_eq!(link.display_url(), "https://qork.me/ka9m");
        assert!(link.is_new);
    }

    #[test]
    fn display_url_falls_back_to_scheme_prefix() {
        let link = ShortLink::from_value(serde_json::json!({
            "shortCode": "ka9m",
            "shortUrl": "qork.me/ka9m",
            "isNew": false
        }))
        .unwrap();
        assert_eq!(link.display_url(), "https://qork.me/ka9m");
        assert!(!link.is_new);
    }

    #[test]
    fn empty_envelope_is_an_error() {
        assert!(ShortLink::from_value(serde_json::json!({})).is_err());
    }
}
