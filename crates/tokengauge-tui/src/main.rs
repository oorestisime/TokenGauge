use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use crossterm::ExecutableCommand;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokengauge_core::{
    ProviderRow, TokenGaugeConfig, load_config, parse_payload_bytes, payload_to_rows, read_cache,
    write_default_config,
};

const BAR_WIDTH: usize = 10;

#[derive(Parser, Debug)]
#[command(version, about = "TokenGauge TUI")]
struct Args {
    #[arg(long, env = "TOKENGAUGE_CONFIG")]
    config: Option<PathBuf>,
}

#[derive(Debug)]
struct AppState {
    rows: Vec<ProviderRow>,
    last_refresh: Instant,
    last_error: Option<String>,
    status_message: Option<String>,
    spinner_index: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            last_refresh: Instant::now(),
            last_error: None,
            status_message: None,
            spinner_index: 0,
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let stdout = io::stdout();
    if !crossterm::tty::IsTty::is_tty(&stdout) {
        return Err(anyhow!("tokengauge-tui must run in a TTY"));
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &args);

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, args: &Args) -> Result<()> {
    let mut state = AppState::default();
    let mut pending_refresh = Some(spawn_refresh(args, false));
    let mut last_cache_poll = Instant::now();

    loop {
        if let Some(receiver) = pending_refresh.as_ref() {
            match receiver.try_recv() {
                Ok(result) => {
                    apply_refresh_result(&mut state, result);
                    pending_refresh = None;
                }
                Err(TryRecvError::Empty) => {
                    state.spinner_index = state.spinner_index.wrapping_add(1);
                }
                Err(TryRecvError::Disconnected) => {
                    state.last_error = Some("refresh thread disconnected".to_string());
                    state.status_message = None;
                    pending_refresh = None;
                }
            }
        }

        if pending_refresh.is_none() && last_cache_poll.elapsed() >= Duration::from_secs(60) {
            last_cache_poll = Instant::now();
            if let Ok(config) = load_config(args.config.clone()) {
                if let Ok(payloads) = read_cache(&config.cache_file) {
                    state.rows = payload_to_rows(payloads, &config);
                    state.last_error = None;
                }
            }
        }

        terminal.draw(|frame| draw_ui(frame, &state, pending_refresh.is_some()))?;

        if event::poll(Duration::from_millis(120))?
            && let Event::Key(key) = event::read()?
        {
            if should_exit(key) {
                break;
            }
            if matches!(key.code, KeyCode::Char('r')) && pending_refresh.is_none() {
                state.status_message = Some("Refreshing…".to_string());
                pending_refresh = Some(spawn_refresh(args, true));
            }
        }

        if pending_refresh.is_none() {
            if let Ok(config) = load_config(args.config.clone()) {
                if state.last_refresh.elapsed() >= Duration::from_secs(config.refresh_secs) {
                    pending_refresh = Some(spawn_refresh(args, false));
                }
            }
        }
    }

    Ok(())
}

fn apply_refresh_result(state: &mut AppState, result: Result<Vec<ProviderRow>>) {
    match result {
        Ok(rows) => {
            state.rows = rows;
            state.last_error = None;
        }
        Err(error) => {
            state.rows.clear();
            state.last_error = Some(error.to_string());
        }
    }
    state.last_refresh = Instant::now();
    state.status_message = None;
}

fn spawn_refresh(args: &Args, force: bool) -> Receiver<Result<Vec<ProviderRow>>> {
    let config_override = args.config.clone();
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let result = fetch_rows_with_config(config_override, force);
        let _ = sender.send(result);
    });

    receiver
}

fn should_exit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
}

fn fetch_rows_with_config(
    config_override: Option<PathBuf>,
    force: bool,
) -> Result<Vec<ProviderRow>> {
    let config_path = config_override.unwrap_or_else(tokengauge_core::default_config_path);
    if !config_path.exists() {
        write_default_config(&config_path)?;
    }

    let config = load_config(Some(config_path))?;
    let payloads = match read_cache(&config.cache_file) {
        Ok(payloads) => payloads,
        Err(_) => refresh_cache(&config, true)?,
    };
    let payloads = if force {
        refresh_cache(&config, true)?
    } else {
        payloads
    };
    Ok(payload_to_rows(payloads, &config))
}

fn refresh_cache(
    config: &TokenGaugeConfig,
    force: bool,
) -> Result<Vec<tokengauge_core::ProviderPayload>> {
    if !force {
        let stale = match fs::metadata(&config.cache_file) {
            Ok(metadata) => metadata
                .modified()
                .ok()
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .map(|age| age >= Duration::from_secs(config.refresh_secs))
                .unwrap_or(true),
            Err(_) => true,
        };
        if !stale {
            return read_cache(&config.cache_file);
        }
    }

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
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            "no error output".to_string()
        };
        return Err(anyhow!("codexbar failed ({}) - {}", output.status, detail));
    }

    let payloads = parse_payload_bytes(&output.stdout)?;
    tokengauge_core::write_cache(&config.cache_file, &payloads)?;
    Ok(payloads)
}

fn percent_color(percent_left: u8) -> Color {
    match percent_left {
        70..=100 => Color::Green,
        40..=69 => Color::Yellow,
        20..=39 => Color::LightRed,
        _ => Color::Red,
    }
}

fn bar_line(percent_used: Option<u8>) -> Line<'static> {
    match percent_used {
        Some(percent) => {
            let percent = percent.min(100);
            let filled = (percent as usize * BAR_WIDTH).div_ceil(100);
            let empty = BAR_WIDTH.saturating_sub(filled);
            let color = percent_color(100 - percent);
            let filled_bar = "█".repeat(filled);
            let empty_bar = "░".repeat(empty);
            Line::from(vec![
                Span::styled(filled_bar, Style::default().fg(color)),
                Span::styled(empty_bar, Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!(" {:>3}%", percent),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ])
        }
        None => Line::from(Span::styled("—", Style::default().fg(Color::DarkGray))),
    }
}

fn draw_ui(frame: &mut ratatui::Frame, state: &AppState, is_refreshing: bool) {
    let size = frame.area();
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(size);

    let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let spinner = spinner_frames[state.spinner_index % spinner_frames.len()];
    let header_label = if is_refreshing {
        "Refreshing"
    } else {
        "TokenGauge Usage"
    };
    let header_text = if is_refreshing {
        format!("{} {}", spinner, header_label)
    } else {
        header_label.to_string()
    };

    let header = Paragraph::new(header_text)
        .style(
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).title("TokenGauge"));
    frame.render_widget(header, layout[0]);

    if state.rows.is_empty() {
        let message = state
            .status_message
            .as_deref()
            .or(state.last_error.as_deref())
            .unwrap_or("No providers returned");
        let empty = Paragraph::new(message)
            .style(Style::default().fg(Color::Red))
            .block(Block::default().borders(Borders::ALL).title("Usage"));
        frame.render_widget(empty, layout[1]);
    } else {
        let table_rows = state.rows.iter().flat_map(|row| {
            let primary = Row::new(vec![
                Cell::from(Span::styled(
                    row.provider.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Cell::from(bar_line(row.session_used)),
                Cell::from(Span::styled(
                    row.session_reset.clone(),
                    Style::default().fg(Color::Gray),
                )),
                Cell::from(bar_line(row.weekly_used)),
                Cell::from(Span::styled(
                    row.weekly_reset.clone(),
                    Style::default().fg(Color::Gray),
                )),
                Cell::from(Span::styled(
                    row.credits.clone(),
                    Style::default().fg(Color::LightGreen),
                )),
                Cell::from(Span::styled(
                    row.source.clone(),
                    Style::default().fg(Color::LightBlue),
                )),
                Cell::from(Span::styled(
                    row.updated.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
            ]);
            let spacer = Row::new(vec![Cell::from(" "); 8]);
            [primary, spacer]
        });

        let table = Table::new(
            table_rows,
            [
                Constraint::Length(12),
                Constraint::Length(18),
                Constraint::Length(20),
                Constraint::Length(18),
                Constraint::Length(20),
                Constraint::Length(10),
                Constraint::Length(18),
                Constraint::Min(8),
            ],
        )
        .header(
            Row::new([
                Cell::from("Provider"),
                Cell::from("Session Used"),
                Cell::from("Session Reset"),
                Cell::from("Weekly Used"),
                Cell::from("Weekly Reset"),
                Cell::from("Credits"),
                Cell::from("Source"),
                Cell::from("Updated"),
            ])
            .style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        )
        .block(Block::default().borders(Borders::ALL).title("Usage"));

        frame.render_widget(table, layout[1]);
    }

    let status_text = state.status_message.as_deref().unwrap_or("Idle");
    let status_color = if state.status_message.is_some() {
        Color::Yellow
    } else {
        Color::DarkGray
    };

    let footer_line = Line::from(vec![
        Span::styled(
            "r",
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" refresh", Style::default().fg(Color::Gray)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "q/esc",
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" quit", Style::default().fg(Color::Gray)),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            status_text,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    let footer = Paragraph::new(footer_line).block(Block::default().borders(Borders::ALL));
    frame.render_widget(footer, layout[2]);
}
