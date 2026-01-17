use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use serde::Serialize;
use tokengauge_core::{
    ProviderRow, TokenGaugeConfig, ensure_cache_dir, load_config, parse_payload_bytes,
    payload_to_rows, read_cache, write_cache, write_default_config,
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

    let rows = payload_to_rows(payloads, &config);
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
            let used = row
                .session_used
                .map(|v| format!("{v}%"))
                .unwrap_or_else(|| "—".into());
            format!("{} {used}", row.provider)
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

fn maybe_refresh(config: &TokenGaugeConfig) -> Result<Vec<tokengauge_core::ProviderPayload>> {
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
        let payloads = fetch_payloads(config)?;
        write_cache(&config.cache_file, &payloads)?;
        Ok(payloads)
    } else {
        read_cache(&config.cache_file)
    }
}

fn fetch_payloads(config: &TokenGaugeConfig) -> Result<Vec<tokengauge_core::ProviderPayload>> {
    let mut command = Command::new(&config.codexbar_bin);
    command
        .arg("usage")
        .arg("--format")
        .arg("json")
        .arg("--source")
        .arg(&config.source);

    let provider_arg = tokengauge_core::provider_argument(&config.providers);
    if let Some(provider_arg) = provider_arg {
        command.arg("--provider").arg(provider_arg);
    }

    let output = command
        .output()
        .with_context(|| format!("failed to run {}", config.codexbar_bin))?;

    if !output.status.success() {
        return Err(anyhow!(
            "codexbar returned non-zero exit: {}",
            output.status
        ));
    }

    parse_payload_bytes(&output.stdout)
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
