//! Integration tests that require codexbar CLI to be installed.
//!
//! These tests call the real codexbar binary and require:
//! - codexbar CLI installed and in PATH
//! - Valid OAuth tokens (for claude/codex providers)
//! - Network access
//!
//! Tests are ignored by default since they require real credentials.
//!
//! Run with:
//!   cargo test --test codexbar_integration -- --ignored
//!
//! Or run all tests including integration:
//!   cargo test --test codexbar_integration -- --include-ignored

use std::process::Command;
use std::time::Duration;
use tokengauge_core::{
    EnabledProvider, ProviderType, fetch_all_providers, fetch_single_provider, load_config,
    parse_payload_bytes, payload_to_rows,
};

/// Check if codexbar is available in PATH
fn codexbar_available() -> bool {
    Command::new("codexbar")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Test fetching claude provider via OAuth
#[test]
#[ignore]
fn test_fetch_claude_oauth() {
    if !codexbar_available() {
        eprintln!("Skipping: codexbar not available");
        return;
    }

    let provider = EnabledProvider {
        name: "claude".to_string(),
        provider_type: ProviderType::OAuth,
        api_key: None,
        env_var: None,
    };

    let result = fetch_single_provider("codexbar", &provider, Duration::from_secs(10));

    match result {
        Ok(payloads) => {
            assert!(!payloads.is_empty(), "Expected at least one payload");
            let payload = &payloads[0];
            assert_eq!(payload.provider, "claude");

            // Check that we got usage data (not an error)
            if payload.has_error() {
                let error = payload.error.as_ref().unwrap();
                eprintln!(
                    "Claude returned error (may need to refresh OAuth): {:?}",
                    error.message
                );
            } else {
                assert!(payload.usage.is_some(), "Expected usage data");
                let usage = payload.usage.as_ref().unwrap();
                println!("Claude usage: {:?}", usage);

                // Verify usage structure
                if let Some(primary) = &usage.primary {
                    println!("  Primary: {}% used", primary.used_percent.unwrap_or(0));
                }
                if let Some(secondary) = &usage.secondary {
                    println!("  Secondary: {}% used", secondary.used_percent.unwrap_or(0));
                }
            }
        }
        Err(e) => {
            eprintln!(
                "Fetch failed (may need to refresh OAuth in Claude app): {}",
                e
            );
            // Don't fail the test - OAuth might need refresh
        }
    }
}

/// Test fetching codex provider via OAuth
#[test]
#[ignore]
fn test_fetch_codex_oauth() {
    if !codexbar_available() {
        eprintln!("Skipping: codexbar not available");
        return;
    }

    let provider = EnabledProvider {
        name: "codex".to_string(),
        provider_type: ProviderType::OAuth,
        api_key: None,
        env_var: None,
    };

    let result = fetch_single_provider("codexbar", &provider, Duration::from_secs(10));

    match result {
        Ok(payloads) => {
            assert!(!payloads.is_empty(), "Expected at least one payload");
            let payload = &payloads[0];
            assert_eq!(payload.provider, "codex");
            println!("Codex payload: provider={}", payload.provider);

            if payload.has_error() {
                let error = payload.error.as_ref().unwrap();
                eprintln!("Codex returned error: {:?}", error.message);
            }
        }
        Err(e) => {
            eprintln!("Fetch failed: {}", e);
        }
    }
}

/// Test that codexbar JSON output parses correctly into our structs
#[test]
#[ignore]
fn test_codexbar_json_parsing() {
    if !codexbar_available() {
        eprintln!("Skipping: codexbar not available");
        return;
    }

    // Call codexbar directly to get raw JSON
    let output = Command::new("codexbar")
        .args([
            "usage",
            "--provider",
            "claude",
            "--source",
            "oauth",
            "--format",
            "json",
            "--json-only",
        ])
        .output()
        .expect("failed to run codexbar");

    if !output.status.success() {
        // Even on failure, codexbar might output JSON with error info
        if !output.stdout.is_empty() {
            let result = parse_payload_bytes(&output.stdout);
            match result {
                Ok(payloads) => {
                    println!("Parsed {} payloads from error response", payloads.len());
                    for p in &payloads {
                        if p.has_error() {
                            println!("  Error: {:?}", p.error);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to parse error JSON: {}", e);
                    eprintln!("Raw stdout: {}", String::from_utf8_lossy(&output.stdout));
                }
            }
        }
        return;
    }

    // Parse the successful output
    let payloads =
        parse_payload_bytes(&output.stdout).expect("Failed to parse codexbar JSON output");

    assert!(!payloads.is_empty(), "Expected at least one payload");

    // Verify we can convert to rows
    let rows = payload_to_rows(payloads.clone());
    println!(
        "Converted {} payloads to {} rows",
        payloads.len(),
        rows.len()
    );

    for row in &rows {
        println!(
            "  {} - session: {:?}%, weekly: {:?}%",
            row.provider, row.session_used, row.weekly_used
        );
    }
}

/// Test fetch_all_providers with real config
#[test]
#[ignore]
fn test_fetch_all_providers_integration() {
    if !codexbar_available() {
        eprintln!("Skipping: codexbar not available");
        return;
    }

    // Try to load real config
    let config = match load_config(None) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("No config found ({}), using default", e);
            tokengauge_core::TokenGaugeConfig::default()
        }
    };

    let enabled = config.providers.enabled_providers();
    println!("Testing with {} enabled providers:", enabled.len());
    for p in &enabled {
        println!("  - {} ({:?})", p.name, p.provider_type);
    }

    let result = fetch_all_providers(&config);

    println!("\nResults:");
    println!("  Successful payloads: {}", result.payloads.len());
    println!("  Errors: {}", result.errors.len());

    for payload in &result.payloads {
        println!(
            "  OK: {} - has_error={}",
            payload.provider,
            payload.has_error()
        );
    }

    for error in &result.errors {
        println!("  ERR: {} - {}", error.provider, error.message);
    }

    // Convert to rows
    let rows = payload_to_rows(result.payloads);
    println!("\nRows:");
    for row in &rows {
        println!(
            "  {} - session: {:?}%, weekly: {:?}%",
            row.provider, row.session_used, row.weekly_used
        );
    }
}

/// Test that the cache file can be read and parsed
#[test]
#[ignore]
fn test_read_existing_cache() {
    use std::path::Path;
    use tokengauge_core::read_cache_full;

    let cache_path = Path::new("/tmp/tokengauge-usage.json");
    if !cache_path.exists() {
        eprintln!("No cache file at {:?}, skipping", cache_path);
        return;
    }

    let cached = read_cache_full(cache_path).expect("Failed to read cache file");

    let payloads = cached.payloads();
    let errors = cached.errors();

    println!("Cache contains:");
    println!("  {} payloads", payloads.len());
    println!("  {} errors", errors.len());

    for payload in payloads {
        println!(
            "  Payload: {} (has_error={})",
            payload.provider,
            payload.has_error()
        );
        if let Some(usage) = &payload.usage
            && let Some(primary) = &usage.primary
        {
            println!("    Primary: {}% used", primary.used_percent.unwrap_or(0));
        }
    }

    for error in errors {
        println!("  Error: {} - {}", error.provider, error.message);
    }

    // Verify we can convert to rows
    let rows = payload_to_rows(payloads.to_vec());
    assert!(rows.len() <= payloads.len());
}
