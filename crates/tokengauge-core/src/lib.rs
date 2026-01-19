use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub primary: Option<UsageWindow>,
    pub secondary: Option<UsageWindow>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub used_percent: Option<u8>,
    pub reset_description: Option<String>,
    pub window_minutes: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Credits {
    pub remaining: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPayload {
    pub provider: String,
    pub version: Option<String>,
    pub source: Option<String>,
    pub usage: Option<UsageSnapshot>,
    pub credits: Option<Credits>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TokenGaugeConfig {
    pub codexbar_bin: String,
    pub source: String,
    pub refresh_secs: u64,
    pub cache_file: PathBuf,
    pub providers: ProviderConfig,
    pub waybar: WaybarConfig,
}

impl Default for TokenGaugeConfig {
    fn default() -> Self {
        Self {
            codexbar_bin: "codexbar".to_string(),
            source: "oauth".to_string(),
            refresh_secs: 600,
            cache_file: PathBuf::from("/tmp/tokengauge-usage.json"),
            providers: ProviderConfig::default(),
            waybar: WaybarConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub codex: bool,
    pub claude: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct WaybarConfig {
    pub window: WaybarWindow,
}

impl Default for WaybarConfig {
    fn default() -> Self {
        Self {
            window: WaybarWindow::Daily,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum WaybarWindow {
    #[default]
    Daily,
    Weekly,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            codex: true,
            claude: true,
        }
    }
}

impl ProviderConfig {
    pub fn is_enabled(&self, provider: &str) -> bool {
        match provider {
            "codex" => self.codex,
            "claude" => self.claude,
            _ => false,
        }
    }

    pub fn enabled_count(&self) -> usize {
        usize::from(self.codex) + usize::from(self.claude)
    }
}

pub fn provider_argument(config: &ProviderConfig) -> Option<&'static str> {
    match (config.codex, config.claude) {
        (true, true) => Some("both"),
        (true, false) => Some("codex"),
        (false, true) => Some("claude"),
        (false, false) => None,
    }
}

#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub provider: String,
    pub session_used: Option<u8>,
    pub session_window_minutes: Option<u32>,
    pub session_reset: String,
    pub weekly_used: Option<u8>,
    pub weekly_window_minutes: Option<u32>,
    pub weekly_reset: String,
    pub credits: String,
    pub source: String,
    pub updated: String,
}

pub fn load_config(path: Option<PathBuf>) -> Result<TokenGaugeConfig> {
    let path = match path {
        Some(path) => path,
        None => default_config_path(),
    };

    let contents = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config at {}", path.display()))?;
    let mut config: TokenGaugeConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse config at {}", path.display()))?;
    if config.codexbar_bin.is_empty() {
        config.codexbar_bin = "codexbar".to_string();
    }
    if config.source.is_empty() {
        config.source = "oauth".to_string();
    }
    if config.cache_file.as_os_str().is_empty() {
        config.cache_file = PathBuf::from("/tmp/tokengauge-usage.json");
    }
    if config.refresh_secs == 0 {
        config.refresh_secs = 600;
    }
    Ok(config)
}

pub fn default_config_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let mut home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            home.push(".config");
            home
        });
    config_dir.join("tokengauge").join("config.toml")
}

pub fn parse_payload(value: serde_json::Value) -> Result<Vec<ProviderPayload>> {
    if value.is_array() {
        serde_json::from_value(value).context("failed to parse provider payload list")
    } else {
        let payload: ProviderPayload =
            serde_json::from_value(value).context("failed to parse provider payload")?;
        Ok(vec![payload])
    }
}

pub fn provider_label(value: &str) -> &str {
    match value {
        "codex" => "Codex",
        "claude" => "Claude",
        "kiro" => "Kiro",
        "gemini" => "Gemini",
        "copilot" => "Copilot",
        "zai" => "z.ai",
        "cursor" => "Cursor",
        "factory" => "Factory",
        "kimi" => "Kimi",
        "kimik2" => "Kimi K2",
        "vertexai" => "Vertex AI",
        "antigravity" => "Antigravity",
        "opencode" => "OpenCode",
        "minimax" => "MiniMax",
        other => other,
    }
}

pub fn format_window(window: Option<UsageWindow>) -> (Option<u8>, Option<u32>, String) {
    if let Some(window) = window {
        let used = window.used_percent.map(|used| used.min(100));
        let minutes = window.window_minutes;
        let reset = window.reset_description.unwrap_or_else(|| "—".to_string());
        (used, minutes, reset)
    } else {
        (None, None, "—".into())
    }
}

pub fn format_updated(value: Option<String>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(&value) {
        let local = timestamp.with_timezone(&Local);
        return local.format("%H:%M").to_string();
    }
    if let Some((_, time_part)) = value.split_once('T') {
        let time = time_part.trim_end_matches('Z');
        let short = time.get(0..5).unwrap_or(time);
        return short.to_string();
    }
    value
}

pub fn payload_to_rows(
    payloads: Vec<ProviderPayload>,
    config: &TokenGaugeConfig,
) -> Vec<ProviderRow> {
    payloads
        .into_iter()
        .filter(|payload| config.providers.is_enabled(&payload.provider))
        .map(provider_to_row)
        .collect()
}

pub struct WeightedAverage {
    pub used_percent: Option<u8>,
    pub window_minutes: u32,
}

pub fn weighted_average(windows: Vec<(Option<u8>, Option<u32>)>) -> Option<u8> {
    let mut total_minutes = 0u64;
    let mut total_weighted = 0u64;

    for (used, minutes) in windows {
        let used = match used {
            Some(value) => value as u64,
            None => continue,
        };
        let minutes = match minutes {
            Some(value) if value > 0 => value as u64,
            _ => continue,
        };
        total_minutes += minutes;
        total_weighted += used * minutes;
    }

    if total_minutes == 0 {
        return None;
    }

    Some(((total_weighted as f64 / total_minutes as f64).round() as u8).min(100))
}

fn provider_to_row(payload: ProviderPayload) -> ProviderRow {
    let usage = payload.usage;
    let (
        session_used,
        session_window,
        session_reset,
        weekly_used,
        weekly_window,
        weekly_reset,
        updated,
    ) = if let Some(usage) = usage {
        let primary = usage.primary;
        let secondary = usage.secondary;
        let updated = format_updated(usage.updated_at);
        let (session_used, session_window, session_reset) = format_window(primary);
        let (weekly_used, weekly_window, weekly_reset) = format_window(secondary);
        (
            session_used,
            session_window,
            session_reset,
            weekly_used,
            weekly_window,
            weekly_reset,
            updated,
        )
    } else {
        (None, None, "—".into(), None, None, "—".into(), "—".into())
    };

    let credits = payload
        .credits
        .and_then(|credits| credits.remaining)
        .map(|remaining| format!("{remaining:.2}"))
        .unwrap_or_else(|| "—".to_string());

    let source = match (payload.version, payload.source) {
        (Some(version), Some(source)) => format!("{version} ({source})"),
        (Some(version), None) => version,
        (None, Some(source)) => source,
        (None, None) => "—".to_string(),
    };

    ProviderRow {
        provider: provider_label(&payload.provider).to_string(),
        session_used,
        session_window_minutes: session_window,
        session_reset,
        weekly_used,
        weekly_window_minutes: weekly_window,
        weekly_reset,
        credits,
        source,
        updated,
    }
}

pub fn read_cache(path: &Path) -> Result<Vec<ProviderPayload>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read cache file {}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&contents).context("cached JSON was invalid")?;
    parse_payload(value)
}

pub fn write_cache(path: &Path, payloads: &[ProviderPayload]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let contents = serde_json::to_string(payloads)?;
    fs::write(path, contents)
        .with_context(|| format!("failed to write cache {}", path.display()))?;
    Ok(())
}

pub fn parse_payload_bytes(bytes: &[u8]) -> Result<Vec<ProviderPayload>> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("codexbar output was not JSON")?;
    parse_payload(value)
}

pub fn ensure_config_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory {}", parent.display()))?;
    }
    Ok(())
}

pub fn write_default_config(path: &Path) -> Result<()> {
    ensure_config_dir(path)?;
    let config = TokenGaugeConfig::default();
    let contents = toml::to_string_pretty(&config)?;
    fs::write(path, contents)
        .with_context(|| format!("failed to write config {}", path.display()))?;
    Ok(())
}

pub fn ensure_cache_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create cache directory {}", parent.display()))?;
    }
    Ok(())
}
