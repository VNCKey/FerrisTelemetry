use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table};
use ratatui::Frame;
use std::sync::atomic::Ordering;

use crate::metrics::histogram::LatencySummary;
use crate::metrics::MetricsStore;

pub fn render_endpoints(f: &mut Frame, area: Rect, metrics: &MetricsStore, selected_index: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(10),   // Table
            Constraint::Length(8), // Route inspector details
        ])
        .split(area);

    let endpoints_map = metrics.endpoints.read().clone();
    let mut endpoints_list: Vec<_> = endpoints_map.into_values().collect();
    endpoints_list.sort_by(|a, b| {
        b.count
            .load(Ordering::Relaxed)
            .cmp(&a.count.load(Ordering::Relaxed))
    });

    let header_cells = [
        "MÉTODO",
        "RUTA HTTP",
        "LLAMADAS",
        "FALLOS",
        "LAT. MEDIA",
        "p99 LAT.",
        "MÍN",
        "MÁX",
        "ESTADO",
    ]
    .iter()
    .map(|h| {
        Cell::from(*h).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    });

    let header = Row::new(header_cells)
        .style(Style::default().bg(Color::DarkGray))
        .height(1);

    let rows: Vec<Row> = if endpoints_list.is_empty() {
        vec![Row::new(vec![
            Cell::from("-"),
            Cell::from(
                "Esperando tráfico HTTP... envía peticiones a http://127.0.0.1:3000 o presiona [s]",
            ),
            Cell::from("0"),
            Cell::from("0"),
            Cell::from("-"),
            Cell::from("-"),
            Cell::from("-"),
            Cell::from("-"),
            Cell::from("Idle"),
        ])]
    } else {
        endpoints_list
            .iter()
            .enumerate()
            .map(|(idx, ep)| {
                let count = ep.count.load(Ordering::Relaxed);
                let errors = ep.errors.load(Ordering::Relaxed);
                let avg_latency = ep.avg_latency_us.load(Ordering::Relaxed);
                let p99_latency = ep.p99_latency_us.load(Ordering::Relaxed);
                let min_latency = ep.min_latency_us.load(Ordering::Relaxed);
                let max_latency = ep.max_latency_us.load(Ordering::Relaxed);

                let method_color = match ep.method.as_str() {
                    "GET" => Color::Green,
                    "POST" => Color::Blue,
                    "PUT" => Color::Yellow,
                    "DELETE" => Color::Red,
                    _ => Color::White,
                };

                let p99_ms = p99_latency as f64 / 1000.0;
                let status_badge = if errors > 0 {
                    ("Error", Color::Red)
                } else if p99_ms > 100.0 {
                    ("Lento", Color::Yellow)
                } else if count > 0 {
                    ("Excelente", Color::Green)
                } else {
                    ("Idle", Color::DarkGray)
                };

                let is_selected = idx == selected_index;
                let row_style = if is_selected {
                    Style::default()
                        .bg(Color::Rgb(30, 45, 60))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                let prefix = if is_selected { "> " } else { "  " };

                Row::new(vec![
                    Cell::from(format!("{}{}", prefix, ep.method))
                        .style(Style::default().fg(method_color)),
                    Cell::from(ep.path.clone()).style(Style::default().fg(Color::White)),
                    Cell::from(count.to_string()).style(Style::default().fg(Color::Cyan)),
                    Cell::from(errors.to_string()).style(Style::default().fg(if errors > 0 {
                        Color::Red
                    } else {
                        Color::DarkGray
                    })),
                    Cell::from(LatencySummary::format_ms(avg_latency))
                        .style(Style::default().fg(Color::White)),
                    Cell::from(LatencySummary::format_ms(p99_latency))
                        .style(Style::default().fg(Color::LightYellow)),
                    Cell::from(if min_latency == u64::MAX {
                        "-".to_string()
                    } else {
                        LatencySummary::format_ms(min_latency)
                    }),
                    Cell::from(LatencySummary::format_ms(max_latency)),
                    Cell::from(status_badge.0).style(Style::default().fg(status_badge.1)),
                ])
                .style(row_style)
                .height(1)
            })
            .collect()
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Percentage(28),
            Constraint::Length(12),
            Constraint::Length(10),
            Constraint::Length(14),
            Constraint::Length(14),
            Constraint::Length(12),
            Constraint::Length(12),
            Constraint::Length(14),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Endpoints y Rutas HTTP Monitoreadas ")
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
    );

    f.render_widget(table, chunks[0]);

    // Bottom Inspector Panel
    let inspector_text = if !endpoints_list.is_empty() && selected_index < endpoints_list.len() {
        let ep = &endpoints_list[selected_index];
        let count = ep.count.load(Ordering::Relaxed);
        let avg_latency = ep.avg_latency_us.load(Ordering::Relaxed);
        let p99_latency = ep.p99_latency_us.load(Ordering::Relaxed);
        let min_latency = ep.min_latency_us.load(Ordering::Relaxed);
        let max_latency = ep.max_latency_us.load(Ordering::Relaxed);

        vec![
            Line::from(vec![
                Span::styled(
                    "Detalles de Ruta: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{} {}", ep.method, ep.path),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" | Total Requests: {}", count),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::styled("Latencias: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!(
                        "Media: {} | p99: {} | Mín: {} | Máx: {}",
                        LatencySummary::format_ms(avg_latency),
                        LatencySummary::format_ms(p99_latency),
                        if min_latency == u64::MAX {
                            "0".to_string()
                        } else {
                            LatencySummary::format_ms(min_latency)
                        },
                        LatencySummary::format_ms(max_latency)
                    ),
                    Style::default().fg(Color::LightCyan),
                ),
            ]),
            Line::from(vec![
                Span::styled("Prueba en terminal: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("curl -s http://127.0.0.1:3000{} | jq", ep.path),
                    Style::default().fg(Color::Green),
                ),
            ]),
            Line::from(vec![Span::styled(
                "Navega entre rutas con [↑] / [↓]",
                Style::default().fg(Color::DarkGray),
            )]),
        ]
    } else {
        vec![
            Line::from(vec![Span::styled(
                "Endpoints disponibles en Axum (:3000):",
                Style::default().fg(Color::Cyan),
            )]),
            Line::from(vec![Span::styled(
                " - GET  /api/v1/users    -> Lee usuarios reales desde PostgreSQL",
                Style::default().fg(Color::White),
            )]),
            Line::from(vec![Span::styled(
                " - GET  /api/v1/compute  -> Test intensivo de CPU (sum of squares)",
                Style::default().fg(Color::White),
            )]),
            Line::from(vec![Span::styled(
                " - GET  /api/v1/plain    -> Payload JSON comparable sin base de datos",
                Style::default().fg(Color::White),
            )]),
            Line::from(vec![Span::styled(
                " - GET  /health          -> Healthcheck de Axum",
                Style::default().fg(Color::White),
            )]),
        ]
    };

    let inspector = Paragraph::new(inspector_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Inspector de Endpoint ")
            .title_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(inspector, chunks[1]);
}
