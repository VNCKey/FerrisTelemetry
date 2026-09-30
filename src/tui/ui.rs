use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Tabs};
use ratatui::Frame;

use super::tabs::{arena, endpoints, overview, system};
use crate::app::App;

pub fn render_ui(f: &mut Frame, app: &App) {
    let size = f.area();

    // Main layout: Header (3) -> Tabs (3) -> Content (Min) -> Footer (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top Header
            Constraint::Length(3), // Navigation Tabs Bar
            Constraint::Min(10),   // Active Tab Body
            Constraint::Length(1), // Bottom Shortcuts Bar
        ])
        .split(size);

    render_header(f, chunks[0], app);
    render_tabs_bar(f, chunks[1], app);
    render_content(f, chunks[2], app);
    render_footer(f, chunks[3], app);
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let header_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Left: Title
    let title_line = Line::from(vec![
        Span::styled(
            "FERRIS",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "TELEMETRY",
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" APM & Framework Arena ", Style::default().fg(Color::White)),
        Span::styled("v0.1.0", Style::default().fg(Color::DarkGray)),
    ]);
    let title = Paragraph::new(title_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(title, header_chunks[0]);

    // Right: Status Badges (Server, DB, LoadGen, Uptime)
    let uptime_secs = app.metrics.uptime_secs();
    let hours = uptime_secs / 3600;
    let mins = (uptime_secs % 3600) / 60;
    let secs = uptime_secs % 60;
    let uptime_str = format!("{:02}:{:02}:{:02}", hours, mins, secs);

    let is_sim_active = app
        .simulator
        .is_active
        .load(std::sync::atomic::Ordering::Relaxed);
    let sim_concurrency = app
        .simulator
        .concurrency
        .load(std::sync::atomic::Ordering::Relaxed);

    let sim_badge = if is_sim_active {
        Span::styled(
            format!("Simulador: ON ({}w)", sim_concurrency),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("Simulador: OFF", Style::default().fg(Color::DarkGray))
    };

    let status_line = Line::from(vec![
        Span::styled(" Axum: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "OK :3000 ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("| DB: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            "SQLite historial | PG Arena ",
            Style::default().fg(Color::LightCyan),
        ),
        Span::styled("| ", Style::default().fg(Color::DarkGray)),
        sim_badge,
        Span::styled(" | Uptime: ", Style::default().fg(Color::DarkGray)),
        Span::styled(uptime_str, Style::default().fg(Color::White)),
    ]);

    let status = Paragraph::new(status_line)
        .alignment(Alignment::Right)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(status, header_chunks[1]);
}

fn render_tabs_bar(f: &mut Frame, area: Rect, app: &App) {
    let tab_titles = vec![
        Line::from(vec![
            Span::raw("1. "),
            Span::styled("Overview", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("2. "),
            Span::styled("Endpoints", Style::default().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("3. "),
            Span::styled(
                "Framework Arena",
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("4. "),
            Span::styled(
                "System Health",
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let tabs = Tabs::new(tab_titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Rgb(50, 70, 90))),
        )
        .select(app.active_tab)
        .style(Style::default().fg(Color::Gray))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));

    f.render_widget(tabs, area);
}

fn render_content(f: &mut Frame, area: Rect, app: &App) {
    match app.active_tab {
        0 => overview::render_overview(f, area, &app.metrics),
        1 => endpoints::render_endpoints(f, area, &app.metrics, app.selected_endpoint_index),
        2 => arena::render_arena(f, area, &app.arena),
        3 => system::render_system(f, area, &app.system_snapshot),
        _ => {}
    }
}

fn render_footer(f: &mut Frame, area: Rect, _app: &App) {
    let shortcuts = Line::from(vec![
        Span::styled(
            " [Tab]/[1-4] ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Navegar Tabs  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[s] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Simulador  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[+]/[-] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Carga  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[a] ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Toggle Servidores  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[m] ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Escenario  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[b] ",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Shootout  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[v] ",
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Matriz  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[r] ",
            Style::default()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Reset  ", Style::default().fg(Color::Gray)),
        Span::styled(
            "[q] ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled("Salir", Style::default().fg(Color::Gray)),
    ]);

    let footer = Paragraph::new(shortcuts).alignment(Alignment::Center);
    f.render_widget(footer, area);
}
