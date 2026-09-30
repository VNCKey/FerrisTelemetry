mod app;
mod arena;
mod db;
mod metrics;
#[allow(dead_code)]
mod server;
mod simulator;
mod system;
mod tui;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::sleep;

use app::App;
use arena::ArenaManager;
use metrics::MetricsStore;
use simulator::LoadGenerator;
use system::SystemMonitor;

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize SQLite Database with WAL mode
    let db_pool = db::init_db("ferris_telemetry.db").await?;

    // 2. Initialize Core Components
    let metrics = MetricsStore::new();
    let arena = ArenaManager::new(Some(db_pool.clone()));
    let simulator = LoadGenerator::new(3000, metrics.clone());
    let system_monitor = SystemMonitor::new();
    arena.attach_system_monitor(system_monitor.clone());

    // 3. Setup Graceful Shutdown Channel
    let (shutdown_tx, _shutdown_rx) = broadcast::channel(1);

    // 4. Auto-launch benchmark targets (Axum :3000, Fiber :8080, FastAPI :8000)
    arena.toggle_companions();

    // Task B: Metrics Sampler (every 1 second)
    let sampler_metrics = metrics.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;
            sampler_metrics.sample_interval();
        }
    });

    // Task C: System Poller & Framework Arena Healthchecker (every 1.5 seconds)
    let poller_system = system_monitor.clone();
    let poller_arena = arena.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1500));
        loop {
            interval.tick().await;
            poller_system.refresh();
            let snap = poller_system.snapshot();
            poller_arena.check_targets_health(&snap).await;
        }
    });

    // Task D: Async Load Generator Loop
    let sim_runner = simulator.clone();
    tokio::spawn(async move {
        sim_runner.start_loop().await;
    });

    // 5. Initialize Terminal UI in Raw Mode
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 6. Create App State
    let mut app = App::new(metrics, arena, simulator, system_monitor);

    // 7. Main Event Loop (~30-60 FPS)
    let tick_rate = Duration::from_millis(50);
    while !app.should_quit {
        terminal.draw(|f| tui::render_ui(f, &app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                app.handle_key_event(key);
            }
        }

        app.update_snapshot();
    }

    // 8. Clean Terminal Teardown
    app.arena.stop_all_companions();
    let _ = shutdown_tx.send(());
    // Give async tasks a moment to terminate cleanly
    sleep(Duration::from_millis(50)).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    println!("FerrisTelemetry APM finalizado con éxito.");
    println!("Base de datos local guardada en: ferris_telemetry.db");

    Ok(())
}
