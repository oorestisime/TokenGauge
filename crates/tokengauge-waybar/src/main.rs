use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use clap::Parser;
use serde::Serialize;
use tokengauge_core::{
    FetchResult, ProviderPayload, ProviderRow, TokenGaugeConfig, WaybarWindow, ensure_cache_dir,
    fetch_all_providers, load_config, payload_to_rows, read_cache, write_cache_full,
    write_default_config,
};

#[derive(Parser, Debug)]
#[command(version, about = "Waybar module for TokenGauge")]
struct Args {
    #[arg(long, env = "TOKENGAUGE_CONFIG")]
    config: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct WaybarOutput {
    text: String,
    tooltip: String,
    class: String,
}

fn format_bar(label: &str, value: Option<u8>) -> String {
    let (bars, percent) = match value {
        Some(percent) => (bar_blocks(percent), format!("{percent}%")),
        None => ("—".to_string(), "—".to_string()),
    };
    format!("{label} {bars} {percent}")
}

fn bar_blocks(percent: u8) -> String {
    match percent.min(100) {
        0..=20 => "▁".to_string(),
        21..=40 => "▁▂".to_string(),
        41..=60 => "▁▂▃".to_string(),
        61..=80 => "▁▂▃▅".to_string(),
        _ => "▁▂▃▅▇".to_string(),
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let config_path = args
        .config
        .unwrap_or_else(tokengauge_core::default_config_path);
    if !config_path.exists() {
        write_default_config(&config_path)?;
    }

    let config = load_config(Some(config_path))?;
    ensure_cache_dir(&config.cache_file)?;

    let payloads = match maybe_refresh(&config) {
        Ok(payloads) => payloads,
        Err(error) => {
            let output = WaybarOutput {
                text: "⟂".into(),
                tooltip: format!("TokenGauge: {error}"),
                class: "tokengauge-error".into(),
            };
            println!("{}", serde_json::to_string(&output)?);
            return Ok(());
        }
    };

    let rows = payload_to_rows(payloads);
    if rows.is_empty() {
        let output = WaybarOutput {
            text: "—".into(),
            tooltip: "TokenGauge: no providers".into(),
            class: "tokengauge-empty".into(),
        };
        println!("{}", serde_json::to_string(&output)?);
        return Ok(());
    }

    let text = rows
        .iter()
        .map(|row| {
            let used = match config.waybar.window {
                WaybarWindow::Daily => row.session_used,
                WaybarWindow::Weekly => row.weekly_used,
            };
            format_bar(&row.provider, used)
        })
        .collect::<Vec<_>>()
        .join("  ");

    let tooltip = rows
        .iter()
        .map(format_tooltip)
        .collect::<Vec<_>>()
        .join("\n");

    let output = WaybarOutput {
        text,
        tooltip,
        class: "tokengauge".into(),
    };

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}

fn maybe_refresh(config: &TokenGaugeConfig) -> Result<Vec<ProviderPayload>> {
    let now = SystemTime::now();
    let stale = match std::fs::metadata(&config.cache_file) {
        Ok(metadata) => metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .map(|age| age >= Duration::from_secs(config.refresh_secs))
            .unwrap_or(true),
        Err(_) => true,
    };

    if stale {
        let FetchResult { payloads, errors } = fetch_all_providers(config);
        // Cache both payloads and errors
        write_cache_full(&config.cache_file, &payloads, &errors)?;
        Ok(payloads)
    } else {
        read_cache(&config.cache_file)
    }
}

fn format_tooltip(row: &ProviderRow) -> String {
    let session = row
        .session_used
        .map(|used| format!("Session {used}% used"))
        .unwrap_or_else(|| "Session —".into());
    let weekly = row
        .weekly_used
        .map(|used| format!("Weekly {used}% used"))
        .unwrap_or_else(|| "Weekly —".into());
    format!(
        "{}: {} (resets {}) | {} (resets {})",
        row.provider, session, row.session_reset, weekly, row.weekly_reset
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------------
    // bar_blocks tests
    // ------------------------------------------------------------------------

    #[test]
    fn bar_blocks_boundaries() {
        // 0-20%
        assert_eq!(bar_blocks(0), "▁");
        assert_eq!(bar_blocks(20), "▁");

        // 21-40%
        assert_eq!(bar_blocks(21), "▁▂");
        assert_eq!(bar_blocks(40), "▁▂");

        // 41-60%
        assert_eq!(bar_blocks(41), "▁▂▃");
        assert_eq!(bar_blocks(60), "▁▂▃");

        // 61-80%
        assert_eq!(bar_blocks(61), "▁▂▃▅");
        assert_eq!(bar_blocks(80), "▁▂▃▅");

        // 81-100%
        assert_eq!(bar_blocks(81), "▁▂▃▅▇");
        assert_eq!(bar_blocks(100), "▁▂▃▅▇");
    }

    #[test]
    fn bar_blocks_clamps_over_100() {
        assert_eq!(bar_blocks(150), "▁▂▃▅▇");
    }

    // ------------------------------------------------------------------------
    // format_bar tests
    // ------------------------------------------------------------------------

    #[test]
    fn format_bar_with_value() {
        let result = format_bar("Claude", Some(42));
        assert!(result.contains("Claude"));
        assert!(result.contains("42%"));
        assert!(result.contains("▁▂▃")); // 41-60% range
    }

    #[test]
    fn format_bar_none() {
        let result = format_bar("Codex", None);
        assert_eq!(result, "Codex — —");
    }

    // ------------------------------------------------------------------------
    // format_tooltip tests
    // ------------------------------------------------------------------------

    #[test]
    fn format_tooltip_full_data() {
        let row = ProviderRow {
            provider: "Claude".to_string(),
            session_used: Some(19),
            session_window_minutes: Some(300),
            session_reset: "Jan 20 at 12:59PM".to_string(),
            weekly_used: Some(12),
            weekly_window_minutes: Some(10080),
            weekly_reset: "Jan 26 at 8:59AM".to_string(),
            credits: "—".to_string(),
            source: "2.1.12 (oauth)".to_string(),
            updated: "07:37".to_string(),
        };
        let tooltip = format_tooltip(&row);
        assert!(tooltip.contains("Claude"));
        assert!(tooltip.contains("Session 19% used"));
        assert!(tooltip.contains("Jan 20 at 12:59PM"));
        assert!(tooltip.contains("Weekly 12% used"));
        assert!(tooltip.contains("Jan 26 at 8:59AM"));
    }

    #[test]
    fn format_tooltip_missing_data() {
        let row = ProviderRow {
            provider: "Codex".to_string(),
            session_used: None,
            session_window_minutes: None,
            session_reset: "—".to_string(),
            weekly_used: None,
            weekly_window_minutes: None,
            weekly_reset: "—".to_string(),
            credits: "—".to_string(),
            source: "—".to_string(),
            updated: "—".to_string(),
        };
        let tooltip = format_tooltip(&row);
        assert!(tooltip.contains("Codex"));
        assert!(tooltip.contains("Session —"));
        assert!(tooltip.contains("Weekly —"));
    }
}
