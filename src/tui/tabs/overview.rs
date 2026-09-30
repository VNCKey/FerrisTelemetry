use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Axis, Block, BorderType, Borders, Chart, Dataset, Gauge, GraphType, Paragraph,
};
use ratatui::Frame;

use crate::metrics::histogram::LatencySummary;
use crate::metrics::MetricsStore;

pub fn render_overview(f: &mut Frame, area: Rect, metrics: &MetricsStore) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // KPI Metric cards
            Constraint::Min(12),   // Braille Real-time Chart
            Constraint::Length(8), // Bottom: Status breakdown & Latency percentiles
        ])
        .split(area);

    // 1. Top KPI Row (4 cards)
    render_kpi_cards(f, chunks[0], metrics);

    // 2. Middle Real-time Chart (RPS & Latency)
    render_chart_section(f, chunks[1], metrics);

    // 3. Bottom Row: Status codes & Latency percentiles
    render_bottom_row(f, chunks[2], metrics);
}

fn render_kpi_cards(f: &mut Frame, area: Rect, metrics: &MetricsStore) {
    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

    let total_reqs = metrics
        .total_requests
        .load(std::sync::atomic::Ordering::Relaxed);
    let current_rps = metrics
        .current_rps
        .load(std::sync::atomic::Ordering::Relaxed);
    let success = metrics
        .success_requests
        .load(std::sync::atomic::Ordering::Relaxed);
    let client_err = metrics
        .client_errors
        .load(std::sync::atomic::Ordering::Relaxed);
    let server_err = metrics
        .server_errors
        .load(std::sync::atomic::Ordering::Relaxed);
    let total_errors = client_err + server_err;

    let error_rate = if total_reqs > 0 {
        (total_errors as f64 / total_reqs as f64) * 100.0
    } else {
        0.0
    };

    let summary = *metrics.current_latency_summary.read();

    // Card 1: Total Requests
    let card1 = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            format!(" {}", total_reqs),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            format!(" 2xx: {} | Err: {}", success, total_errors),
            Style::default().fg(Color::DarkGray),
        )]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Total Peticiones ")
            .title_style(Style::default().fg(Color::LightCyan)),
    );
    f.render_widget(card1, cards[0]);

    // Card 2: Current RPS
    let rps_color = if current_rps > 1000 {
        Color::Magenta
    } else if current_rps > 100 {
        Color::Green
    } else {
        Color::Yellow
    };
    let card2 = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            format!(" {} Req/s", current_rps),
            Style::default().fg(rps_color).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            " Tasa de rendimiento instantánea",
            Style::default().fg(Color::DarkGray),
        )]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Throughput Actual ")
            .title_style(Style::default().fg(Color::LightGreen)),
    );
    f.render_widget(card2, cards[1]);

    // Card 3: Error Rate
    let err_color = if error_rate > 5.0 {
        Color::Red
    } else if error_rate > 0.5 {
        Color::Yellow
    } else {
        Color::Green
    };
    let card3 = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            format!(" {:.2}%", error_rate),
            Style::default().fg(err_color).add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            format!(" Fallos: {} / {}", total_errors, total_reqs),
            Style::default().fg(Color::DarkGray),
        )]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Tasa de Error ")
            .title_style(Style::default().fg(Color::LightYellow)),
    );
    f.render_widget(card3, cards[2]);

    // Card 4: p95 Latency
    let p95_str = LatencySummary::format_ms(summary.p95_us);
    let card4 = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            format!(" {}", p95_str),
            Style::default()
                .fg(Color::LightBlue)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            format!(
                " p50: {} | p99: {}",
                LatencySummary::format_ms(summary.p50_us),
                LatencySummary::format_ms(summary.p99_us)
            ),
            Style::default().fg(Color::DarkGray),
        )]),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Latencia (p95) ")
            .title_style(Style::default().fg(Color::LightBlue)),
    );
    f.render_widget(card4, cards[3]);
}

fn render_chart_section(f: &mut Frame, area: Rect, metrics: &MetricsStore) {
    let rps_data = metrics.rps_history.read().clone();
    let rps_slice: Vec<(f64, f64)> = rps_data.into_iter().collect();

    let max_y = rps_slice.iter().map(|(_, y)| *y).fold(10.0, f64::max) * 1.15;

    let dataset = Dataset::default()
        .name("Req/s (Últimos 60s)")
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(Color::Cyan))
        .data(&rps_slice);

    let x_labels = vec![
        Span::styled("-60s", Style::default().fg(Color::DarkGray)),
        Span::styled("-30s", Style::default().fg(Color::DarkGray)),
        Span::styled("Ahora", Style::default().fg(Color::White)),
    ];

    let y_step = max_y / 3.0;
    let y_labels = vec![
        Span::styled("0", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.0}", y_step),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{:.0}", y_step * 2.0),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{:.0}", max_y), Style::default().fg(Color::Cyan)),
    ];

    let chart = Chart::new(vec![dataset])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Curva de Rendimiento en Tiempo Real (Req/s con caracteres Braille) ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .x_axis(
            Axis::default()
                .title("Ventana de Tiempo")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, 59.0])
                .labels(x_labels),
        )
        .y_axis(
            Axis::default()
                .title("Req/s")
                .style(Style::default().fg(Color::Gray))
                .bounds([0.0, max_y])
                .labels(y_labels),
        );

    f.render_widget(chart, area);
}

fn render_bottom_row(f: &mut Frame, area: Rect, metrics: &MetricsStore) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let total = metrics
        .total_requests
        .load(std::sync::atomic::Ordering::Relaxed)
        .max(1);
    let s2xx = metrics
        .success_requests
        .load(std::sync::atomic::Ordering::Relaxed);
    let s4xx = metrics
        .client_errors
        .load(std::sync::atomic::Ordering::Relaxed);
    let s5xx = metrics
        .server_errors
        .load(std::sync::atomic::Ordering::Relaxed);

    let p2xx = (s2xx as f64 / total as f64 * 100.0) as u16;
    let p4xx = (s4xx as f64 / total as f64 * 100.0) as u16;
    let p5xx = (s5xx as f64 / total as f64 * 100.0) as u16;

    // Left: Status Code Distribution
    let left_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Códigos HTTP de Respuesta ")
        .title_style(Style::default().fg(Color::Green));
    f.render_widget(left_block, chunks[0]);

    let gauge_layout = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(chunks[0]);

    let g1 = Gauge::default()
        .gauge_style(Style::default().fg(Color::Green))
        .percent(p2xx.min(100))
        .label(format!("2xx OK: {} ({}%)", s2xx, p2xx));
    f.render_widget(g1, gauge_layout[0]);

    let g2 = Gauge::default()
        .gauge_style(Style::default().fg(Color::Yellow))
        .percent(p4xx.min(100))
        .label(format!("4xx Client: {} ({}%)", s4xx, p4xx));
    f.render_widget(g2, gauge_layout[1]);

    let g3 = Gauge::default()
        .gauge_style(Style::default().fg(Color::Red))
        .percent(p5xx.min(100))
        .label(format!("5xx Server: {} ({}%)", s5xx, p5xx));
    f.render_widget(g3, gauge_layout[2]);

    // Right: Detailed Latency Percentiles Table
    let summary = *metrics.current_latency_summary.read();
    let text = vec![
        Line::from(vec![
            Span::styled(" Min: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<10}", LatencySummary::format_ms(summary.min_us)),
                Style::default().fg(Color::White),
            ),
            Span::styled(" p50 (Mediana): ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<10}", LatencySummary::format_ms(summary.p50_us)),
                Style::default().fg(Color::Green),
            ),
        ]),
        Line::from(vec![
            Span::styled(" p90: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<10}", LatencySummary::format_ms(summary.p90_us)),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(" p95: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<10}", LatencySummary::format_ms(summary.p95_us)),
                Style::default().fg(Color::LightYellow),
            ),
        ]),
        Line::from(vec![
            Span::styled(" p99: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<10}", LatencySummary::format_ms(summary.p99_us)),
                Style::default().fg(Color::LightRed),
            ),
            Span::styled(" Max: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{:<10}", LatencySummary::format_ms(summary.max_us)),
                Style::default().fg(Color::Red),
            ),
        ]),
        Line::from(vec![Span::styled(
            format!(" Muestras en ventana actual: {}", summary.sample_count),
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let right_p = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Desglose de Percentiles de Latencia ")
            .title_style(Style::default().fg(Color::LightBlue)),
    );
    f.render_widget(right_p, chunks[1]);
}
