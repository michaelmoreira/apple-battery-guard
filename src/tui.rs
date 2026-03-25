//! Dashboard ratatui — mostra estado da bateria em tempo real.

use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph},
    Terminal,
};

use crate::battery::{Battery, BatteryStatus};

pub fn run_tui() -> Result<(), io::Error> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<(), io::Error> {
    let battery = Battery::detect().ok();
    let mut last_refresh = Instant::now();
    let mut status: Option<BatteryStatus> = None;
    let refresh_interval = Duration::from_secs(5);

    loop {
        // Atualiza estado a cada 5s
        if last_refresh.elapsed() >= refresh_interval {
            status = battery.as_ref().and_then(|b| b.status().ok());
            last_refresh = Instant::now();
        }

        terminal.draw(|f| draw(f, &status))?;

        // Polling de teclas com timeout de 500ms
        if event::poll(Duration::from_millis(500))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn draw(f: &mut ratatui::Frame, status: &Option<BatteryStatus>) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3), // título
            Constraint::Length(4), // gauge de carga
            Constraint::Length(3), // estado / threshold
            Constraint::Min(0),    // espaço livre
            Constraint::Length(1), // keybindings
        ])
        .split(area);

    // ── Título ────────────────────────────────────────────────────────────────
    let title = Paragraph::new("apple-battery-guard")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(title, chunks[0]);

    // ── Gauge de carga ────────────────────────────────────────────────────────
    let (capacity, gauge_color) = match status {
        Some(s) => {
            let color = capacity_color(s.capacity);
            (s.capacity, color)
        }
        None => (0, Color::DarkGray),
    };

    let gauge = Gauge::default()
        .block(Block::default().title(" Carga ").borders(Borders::ALL))
        .gauge_style(Style::default().fg(gauge_color))
        .percent(capacity as u16)
        .label(format!("{capacity}%"));
    f.render_widget(gauge, chunks[1]);

    // ── Estado e threshold ────────────────────────────────────────────────────
    let info_text = match status {
        Some(s) => {
            let status_color = status_color(&s.status);
            let threshold_str = s
                .charge_control_end_threshold
                .map(|t| format!("{t}%"))
                .unwrap_or_else(|| "não suportado".to_string());
            vec![Line::from(vec![
                Span::raw("Estado: "),
                Span::styled(&s.status, Style::default().fg(status_color)),
                Span::raw(format!("   Threshold: {threshold_str}")),
            ])]
        }
        None => vec![Line::from(Span::styled(
            "Bateria não detetada",
            Style::default().fg(Color::Red),
        ))],
    };

    let info = Paragraph::new(info_text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Left);
    f.render_widget(info, chunks[2]);

    // ── Keybindings ───────────────────────────────────────────────────────────
    let help = Paragraph::new("  q / Esc: sair")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Left);
    f.render_widget(help, chunks[4]);
}

fn capacity_color(pct: u8) -> Color {
    match pct {
        0..=20 => Color::Red,
        21..=40 => Color::LightRed,
        41..=60 => Color::Yellow,
        61..=80 => Color::Green,
        _ => Color::LightGreen,
    }
}

fn status_color(status: &str) -> Color {
    match status {
        "Charging" => Color::Green,
        "Discharging" => Color::Yellow,
        "Full" => Color::Cyan,
        "Not charging" => Color::Gray,
        _ => Color::White,
    }
}
