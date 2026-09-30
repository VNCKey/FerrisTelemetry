use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Gauge, Paragraph};
use ratatui::Frame;

use crate::arena::ArenaManager;
use crate::metrics::histogram::LatencySummary;

pub fn render_arena(f: &mut Frame, area: Rect, arena: &ArenaManager) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // Target cards (Rust, Go, Python)
            Constraint::Length(3), // Benchmark Shootout Status & Progress Bar
            Constraint::Min(12),   // Comparative Visual Bars (RPS, Latency, RAM, Efficiency)
            Constraint::Length(7), // Live Arena Execution Log & Quickstart
        ])
        .split(area);

    let targets = arena.targets.read().clone();

    // 1. Target Cards
    render_target_cards(f, chunks[0], &targets);

    // 2. Shootout Progress Bar
    render_shootout_bar(f, chunks[1], arena);

    // 3. Comparative Bars
    render_comparative_bars(f, chunks[2], &targets);

    // 4. Live Logs
    render_execution_log(f, chunks[3], arena);
}

fn render_target_cards(f: &mut Frame, area: Rect, targets: &[crate::arena::FrameworkTarget]) {
    let constraints: Vec<Constraint> = (0..targets.len())
        .map(|_| Constraint::Ratio(1, targets.len().max(1) as u32))
        .collect();
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for (i, t) in targets.iter().enumerate() {
        let status_span = if t.is_online {
            Span::styled(
                "ONLINE",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled("OFFLINE", Style::default().fg(Color::DarkGray))
        };

        let ram_str = if t.is_online {
            format!("{:.1} MB", t.ram_mb)
        } else {
            "-".to_string()
        };

        let cpu_str = if t.cpu_pct > 0.0 {
            format!("{:.1}% (activo)", t.cpu_pct)
        } else {
            "0.0% (idle)".to_string()
        };

        let content = vec![
            Line::from(vec![
                Span::styled(
                    format!("Puerto :{} ", t.port),
                    Style::default().fg(Color::DarkGray),
                ),
                status_span,
            ]),
            Line::from(vec![Span::styled(
                format!("RAM: {} | CPU: {}", ram_str, cpu_str),
                Style::default().fg(Color::White),
            )]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(format!(" {} ", t.name))
            .title_style(Style::default().fg(t.color).add_modifier(Modifier::BOLD));

        f.render_widget(Paragraph::new(content).block(block), cols[i]);
    }
}

fn render_shootout_bar(f: &mut Frame, area: Rect, arena: &ArenaManager) {
    let is_running = *arena.is_running_benchmark.read();
    let progress = *arena.benchmark_progress.read();
    let scenario = *arena.current_scenario.read();

    if is_running {
        let pct = (progress * 100.0) as u16;
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::NONE))
            .gauge_style(
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
            )
            .percent(pct.min(100))
            .label(format!("{} - progreso {}%", scenario.label(), pct));
        f.render_widget(gauge, area);
    } else {
        let prompt = Paragraph::new(vec![Line::from(vec![
            Span::styled(
                "ACCION: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("Presiona ", Style::default().fg(Color::White)),
            Span::styled(
                "[a]",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" servidores | ", Style::default().fg(Color::White)),
            Span::styled(
                "[m]",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" escenario: {} | ", scenario.label()),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                "[b]",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" benchmark | ", Style::default().fg(Color::White)),
            Span::styled(
                "[v]",
                Style::default()
                    .fg(Color::LightMagenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" matriz 8..256.", Style::default().fg(Color::White)),
        ])])
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .style(Style::default().bg(Color::Rgb(20, 25, 35))),
        );
        f.render_widget(prompt, area);
    }
}

fn render_comparative_bars(f: &mut Frame, area: Rect, targets: &[crate::arena::FrameworkTarget]) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(25), // RPS
            Constraint::Percentage(25), // Latency p99
            Constraint::Percentage(25), // RAM Consumption
            Constraint::Percentage(25), // Efficiency Score
        ])
        .split(area);

    let max_rps = targets.iter().map(|t| t.current_rps).fold(10.0, f64::max);
    let max_ram = targets.iter().map(|t| t.ram_mb).fold(10.0, f64::max);
    let max_lat = targets.iter().map(|t| t.p99_us).fold(1000, u64::max) as f64;
    let max_eff = targets
        .iter()
        .map(|t| t.efficiency_score())
        .fold(1.0, f64::max);

    // Section 1: Throughput (Req/s)
    render_metric_row(
        f,
        sections[0],
        "THROUGHPUT (Req/s - Mayor es mejor)",
        targets,
        |t| {
            let val = t.current_rps;
            let pct = if max_rps > 0.0 {
                (val / max_rps * 100.0) as u16
            } else {
                0
            };
            let label = if val > 0.0 {
                format!("{:.0} req/s ±{:.1}%", val, t.rps_variation_pct)
            } else {
                "Listo ([b] test)".to_string()
            };
            (pct, label, t.color)
        },
    );

    // Section 2: Latencia p99
    render_metric_row(
        f,
        sections[1],
        "LATENCIA p99 (Microsegundos - Menor es mejor)",
        targets,
        |t| {
            let val = t.p99_us;
            let pct = if max_lat > 0.0 {
                (val as f64 / max_lat * 100.0) as u16
            } else {
                0
            };
            let label = if val > 0 {
                LatencySummary::format_ms(val)
            } else {
                "-".to_string()
            };
            (pct, label, Color::LightYellow)
        },
    );

    // Section 3: Memoria RAM
    render_metric_row(
        f,
        sections[2],
        "MEMORIA RAM RSS (Menor es mejor)",
        targets,
        |t| {
            let val = t.ram_mb;
            let pct = if max_ram > 0.0 {
                (val / max_ram * 100.0) as u16
            } else {
                0
            };
            (pct, format!("{:.1} MB", val), Color::LightGreen)
        },
    );

    // Section 4: Efficiency
    render_metric_row(
        f,
        sections[3],
        "EFICIENCIA (Req/s por MB de RAM - Mayor es mejor)",
        targets,
        |t| {
            let val = t.efficiency_score();
            let pct = if max_eff > 0.0 {
                (val / max_eff * 100.0) as u16
            } else {
                0
            };
            (pct, format!("{:.1} score", val), Color::Magenta)
        },
    );
}

fn render_metric_row<F>(
    f: &mut Frame,
    area: Rect,
    title: &str,
    targets: &[crate::arena::FrameworkTarget],
    formatter: F,
) where
    F: Fn(&crate::arena::FrameworkTarget) -> (u16, String, Color),
{
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(Color::LightCyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let constraints: Vec<Constraint> = (0..targets.len())
        .map(|_| Constraint::Ratio(1, targets.len().max(1) as u32))
        .collect();
    let bars = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    for (i, t) in targets.iter().enumerate() {
        if !t.is_online {
            let offline_text = Paragraph::new(format!("{}: Offline", t.name))
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(offline_text, bars[i]);
            continue;
        }

        let (pct, label, color) = formatter(t);
        let g = Gauge::default()
            .gauge_style(Style::default().fg(color))
            .percent(pct.min(100))
            .label(format!("{}: {}", t.name, label));
        f.render_widget(g, bars[i]);
    }
}

fn render_execution_log(f: &mut Frame, area: Rect, arena: &ArenaManager) {
    let logs = arena.execution_log.read().clone();
    let visible_logs: Vec<Line> = logs
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(|line| {
            let color = if line.contains("OK") || line.contains("GANADOR") {
                Color::Green
            } else if line.contains("WARN") {
                Color::Yellow
            } else if line.contains("RUN") || line.contains("BENCH") {
                Color::Cyan
            } else {
                Color::DarkGray
            };
            Line::from(vec![Span::styled(
                format!("  {}", line),
                Style::default().fg(color),
            )])
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Consola de Resultados & Guia de Servidores ")
        .title_style(Style::default().fg(Color::Gray));

    let p = Paragraph::new(visible_logs).block(block);
    f.render_widget(p, area);
}
