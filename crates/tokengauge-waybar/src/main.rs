use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use anyhow::{Result};
use clap::Parser;
use serde::Serialize;
use tokengauge_core::{
    FetchResult, ProviderPayload, ProviderRow, TokenGaugeConfig, WaybarWindow,
    ensure_cache_dir, fetch_all_providers, load_config, payload_to_rows,
    read_cache, write_cache_full, write_default_config,
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
