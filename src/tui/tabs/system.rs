use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders, Cell, Gauge, Paragraph, Row, Table};
use ratatui::Frame;

use crate::system::SystemSnapshot;

pub fn render_system(f: &mut Frame, area: Rect, snapshot: &SystemSnapshot) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // System Overview: Global CPU, RAM, Swap Gauges
            Constraint::Min(6),    // Cores breakdown
            Constraint::Length(9), // Monitored Process Details Table
        ])
        .split(area);

    // 1. System Overview (Global CPU & RAM)
    render_system_gauges(f, chunks[0], snapshot);

    // 2. Per-Core CPU breakdown
    render_cpu_cores(f, chunks[1], snapshot);

    // 3. Process Table
    render_process_table(f, chunks[2], snapshot);
}

fn render_system_gauges(f: &mut Frame, area: Rect, snapshot: &SystemSnapshot) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Recursos Globales del Host Linux ")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    // CPU Gauge
    let cpu_pct = snapshot.cpu_global.clamp(0.0, 100.0) as u16;
    let cpu_gauge = Gauge::default()
        .gauge_style(Style::default().fg(if cpu_pct > 80 {
            Color::Red
        } else if cpu_pct > 50 {
            Color::Yellow
        } else {
            Color::Green
        }))
        .percent(cpu_pct)
        .label(format!(
            "CPU Total: {:.1}% ({} núcleos activos)",
            snapshot.cpu_global,
            snapshot.cpu_cores.len()
        ));
    f.render_widget(cpu_gauge, rows[0]);

    // RAM Gauge
    let mem_pct = snapshot.memory_percent.clamp(0.0, 100.0) as u16;
    let mem_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(mem_pct)
        .label(format!(
            "Memoria RAM: {:.1} MB / {:.1} MB ({:.1}%)",
            snapshot.memory_used_mb, snapshot.memory_total_mb, snapshot.memory_percent
        ));
    f.render_widget(mem_gauge, rows[1]);

    // Swap Gauge
    let swap_pct = if snapshot.swap_total_mb > 0.0 {
        ((snapshot.swap_used_mb / snapshot.swap_total_mb) * 100.0).clamp(0.0, 100.0) as u16
    } else {
        0
    };
    let swap_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::LightMagenta))
        .percent(swap_pct)
        .label(format!(
            "Swap: {:.1} MB / {:.1} MB ({}%)",
            snapshot.swap_used_mb, snapshot.swap_total_mb, swap_pct
        ));
    f.render_widget(swap_gauge, rows[2]);
}

fn render_cpu_cores(f: &mut Frame, area: Rect, snapshot: &SystemSnapshot) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Uso de CPU por Núcleo (Kernel Linux) ")
        .title_style(Style::default().fg(Color::LightGreen));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let cores_count = snapshot.cpu_cores.len();
    if cores_count == 0 {
        let p = Paragraph::new("Detectando núcleos de CPU...")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(p, inner);
        return;
    }

    // Grid layout: split into columns
    let num_cols = if cores_count > 8 {
        4
    } else if cores_count > 4 {
        2
    } else {
        1
    };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Percentage(100 / num_cols as u16);
            num_cols
        ])
        .split(inner);

    let cores_per_col = cores_count.div_ceil(num_cols);

    for col_idx in 0..num_cols {
        let start_core = col_idx * cores_per_col;
        let end_core = (start_core + cores_per_col).min(cores_count);

        let col_rect = cols[col_idx];
        let num_items = end_core - start_core;
        if num_items == 0 {
            continue;
        }

        let core_rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(1); num_items])
            .split(col_rect);

        for (i, core_idx) in (start_core..end_core).enumerate() {
            let usage = snapshot.cpu_cores[core_idx];
            let pct = usage.clamp(0.0, 100.0) as u16;
            let color = if pct > 80 {
                Color::Red
            } else if pct > 50 {
                Color::Yellow
            } else {
                Color::Green
            };

            let g = Gauge::default()
                .gauge_style(Style::default().fg(color))
                .percent(pct)
                .label(format!("Core #{:<2}: {:>4.1}%", core_idx, usage));
            f.render_widget(g, core_rows[i]);
        }
    }
}

fn render_process_table(f: &mut Frame, area: Rect, snapshot: &SystemSnapshot) {
    let header_cells = [
        "PROCESO / FRAMEWORK",
        "PID",
        "RAM RESIDENTE (RSS)",
        "USO CPU",
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

    let format_proc_row = |p: &crate::system::ProcessMetric, color: Color| -> Row {
        let (status_str, status_color) = if p.is_running {
            ("Activo", Color::Green)
        } else {
            ("No detectado", Color::DarkGray)
        };

        let pid_str = if p.is_running {
            p.pid.to_string()
        } else {
            "-".to_string()
        };
        let ram_str = if p.is_running {
            format!("{:.2} MB", p.memory_mb)
        } else {
            "-".to_string()
        };
        let cpu_str = if p.is_running {
            format!("{:.1}%", p.cpu_usage)
        } else {
            "-".to_string()
        };

        Row::new(vec![
            Cell::from(p.name.clone())
                .style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Cell::from(pid_str).style(Style::default().fg(Color::White)),
            Cell::from(ram_str).style(Style::default().fg(Color::Yellow)),
            Cell::from(cpu_str).style(Style::default().fg(Color::White)),
            Cell::from(status_str).style(Style::default().fg(status_color)),
        ])
        .height(1)
    };

    let rows = vec![
        format_proc_row(&snapshot.self_process, Color::Cyan),
        format_proc_row(&snapshot.rust_process, Color::LightCyan),
        format_proc_row(&snapshot.actix_process, Color::Magenta),
        format_proc_row(&snapshot.go_process, Color::LightBlue),
        format_proc_row(&snapshot.python_process, Color::Yellow),
    ];

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(35),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Procesos Monitoreados por FerrisTelemetry ")
            .title_style(Style::default().fg(Color::Yellow)),
    );

    f.render_widget(table, area);
}
