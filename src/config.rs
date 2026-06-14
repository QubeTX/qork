//! Runtime configuration for qork.

/// Default API base. Overridable with `--api-base` or the `QORK_API_BASE`
/// environment variable (for self-hosting or local testing against a dev
/// server). The CLI appends `/api/shorten` to this.
pub const DEFAULT_API_BASE: &str = "https://qork.me";

/// Output format for a successful shorten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// Print just the short URL (pipeable).
    Plain,
    /// Print the raw JSON envelope from the API.
    Json,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Whether to use ANSI color (status lines + errors). Plain shortens
    /// always print the bare URL to stdout regardless, so output stays
    /// pipeable.
    pub use_colors: bool,
    /// Output format for a successful shorten.
    pub format: OutputFormat,
    /// API base URL, already trimmed of a trailing slash.
    pub api_base: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            use_colors: true,
            format: OutputFormat::Plain,
            api_base: DEFAULT_API_BASE.to_string(),
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_colors(mut self, colors: bool) -> Self {
        self.use_colors = colors;
        self
    }

    pub fn with_json(mut self, json: bool) -> Self {
        if json {
            self.format = OutputFormat::Json;
        }
        self
    }

    /// Override the API base. An empty / whitespace value is ignored (keeps
    /// the default). A trailing slash is stripped so `{base}/api/shorten`
    /// never doubles up.
    pub fn with_api_base(mut self, base: Option<String>) -> Self {
        if let Some(base) = base {
            let trimmed = base.trim().trim_end_matches('/');
            if !trimmed.is_empty() {
                self.api_base = trimmed.to_string();
            }
        }
        self
    }

    pub fn is_json(&self) -> bool {
        self.format == OutputFormat::Json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_is_qork_me() {
        assert_eq!(Config::new().api_base, "https://qork.me");
    }

    #[test]
    fn api_base_strips_trailing_slash() {
        let c = Config::new().with_api_base(Some("https://qork.me/".to_string()));
        assert_eq!(c.api_base, "https://qork.me");
    }

    #[test]
    fn blank_api_base_keeps_default() {
        let c = Config::new().with_api_base(Some("   ".to_string()));
        assert_eq!(c.api_base, "https://qork.me");
    }

    #[test]
    fn with_json_sets_format() {
        assert!(Config::new().with_json(true).is_json());
        assert!(!Config::new().with_json(false).is_json());
    }
}
