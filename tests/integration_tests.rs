use ferris_telemetry::arena::FrameworkTarget;
use ferris_telemetry::db::{self, init_db};
use ferris_telemetry::metrics::{LatencySummary, MetricsStore};
use ratatui::style::Color;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn test_metrics_store_recording_and_sampling() {
    let store = MetricsStore::new();

    // Record some requests
    store.record_request("GET", "/api/v1/users", 200, 1500); // 1.5ms
    store.record_request("GET", "/api/v1/users", 200, 2500); // 2.5ms
    store.record_request("POST", "/api/v1/users", 201, 3000); // 3.0ms
    store.record_request("GET", "/api/v1/unknown", 404, 500); // 404
    store.record_request("GET", "/api/v1/error", 500, 1000); // 500

    assert_eq!(store.total_requests.load(Ordering::Relaxed), 5);
    assert_eq!(store.success_requests.load(Ordering::Relaxed), 3);
    assert_eq!(store.client_errors.load(Ordering::Relaxed), 1);
    assert_eq!(store.server_errors.load(Ordering::Relaxed), 1);

    // Check endpoints
    {
        let endpoints = store.endpoints.read();
        assert!(endpoints.contains_key("GET /api/v1/users"));
        let users_ep = endpoints.get("GET /api/v1/users").unwrap();
        assert_eq!(users_ep.count.load(Ordering::Relaxed), 2);
        assert_eq!(users_ep.errors.load(Ordering::Relaxed), 0);
    }

    // Trigger sample interval
    store.sample_interval();

    // Verify RPS
    assert_eq!(store.current_rps.load(Ordering::Relaxed), 5);

    // Verify RPS history RingBuffer has 60 items
    {
        let rps_hist = store.rps_history.read();
        assert_eq!(rps_hist.len(), 60);
        let last = rps_hist.back().unwrap();
        assert_eq!(last.1, 5.0);
    }

    // Reset store
    store.reset();
    assert_eq!(store.total_requests.load(Ordering::Relaxed), 0);
    assert_eq!(store.current_rps.load(Ordering::Relaxed), 0);
}

#[test]
fn test_latency_summary_calculations() {
    let samples: Vec<u64> = vec![100, 200, 300, 400, 500, 600, 700, 800, 900, 1000];
    let summary = LatencySummary::from_samples(samples);

    assert_eq!(summary.sample_count, 10);
    assert_eq!(summary.min_us, 100);
    assert_eq!(summary.max_us, 1000);
    assert_eq!(summary.avg_us, 550);
    assert_eq!(summary.p50_us, 500);
    assert_eq!(summary.p90_us, 900);
    assert_eq!(summary.p99_us, 1000);

    // Test formatter
    assert_eq!(LatencySummary::format_ms(450), "450 µs");
    assert_eq!(LatencySummary::format_ms(2500), "2.50 ms");
    assert_eq!(LatencySummary::format_ms(1_500_000), "1.50 s");
}

#[test]
fn test_framework_target_efficiency() {
    let mut target = FrameworkTarget::new("rust", "Rust Axum", 3000, Color::Cyan);
    target.is_online = true;
    target.current_rps = 10000.0;
    target.ram_mb = 20.0;

    let efficiency = target.efficiency_score();
    assert_eq!(efficiency, 500.0);

    // Offline target should have 0 score
    target.is_online = false;
    assert_eq!(target.efficiency_score(), 0.0);
}

#[tokio::test]
async fn test_sqlite_db_initialization_and_operations() {
    // Unique test db file in temp
    let test_db = format!(
        "/tmp/test_ferris_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let pool = init_db(&test_db).await.expect("Failed to init SQLite db");

    // Fetch seeded users
    let users = db::get_users(&pool).await.expect("Failed to get users");
    assert!(!users.is_empty(), "Database should have seeded users");
    assert!(users.iter().any(|u| u.username == "ferris_crab"));

    // Save benchmark result
    db::save_benchmark_result(&pool, "🦀 Rust (Axum)", 15420.5, 0.45, 14.8)
        .await
        .expect("Failed to save benchmark result");

    db::save_benchmark_run(
        &pool,
        "🦀 Rust (Axum)",
        "HTTP/JSON (sin DB)",
        15420.5,
        1.2,
        0.15,
        0.30,
        0.45,
        10_000,
        0,
        4,
        3.0,
        14.8,
    )
    .await
    .expect("Failed to save rich benchmark run");

    // Clean up
    let _ = std::fs::remove_file(test_db);
}

#[tokio::test]
async fn test_axum_server_live_requests() {
    let test_db = format!(
        "/tmp/test_axum_{}.db",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let pool = init_db(&test_db).await.expect("Failed to init SQLite");
    let metrics = MetricsStore::new();
    let (shutdown_tx, shutdown_rx) = tokio::sync::broadcast::channel(1);

    let s_metrics = metrics.clone();
    let s_pool = pool.clone();
    let port = 3099; // Test port
    tokio::spawn(async move {
        let _ = ferris_telemetry::server::run_server(s_metrics, s_pool, port, shutdown_rx).await;
    });

    // Give server 50ms to bind
    tokio::time::sleep(std::time::Duration::from_millis(60)).await;

    let client = reqwest::Client::new();
    let base_url = format!("http://127.0.0.1:{}", port);

    // 1. Test /health
    let res = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .expect("Failed /health");
    assert!(res.status().is_success());

    // 2. Test /api/v1/users
    let res = client
        .get(format!("{}/api/v1/users", base_url))
        .send()
        .await
        .expect("Failed /api/v1/users");
    assert!(res.status().is_success());
    let body: serde_json::Value = res.json().await.expect("Failed to parse JSON");
    assert_eq!(body["source"], "sqlite");
    assert!(!body["users"].as_array().unwrap().is_empty());

    // 3. Test /api/v1/compute
    let res = client
        .get(format!("{}/api/v1/compute", base_url))
        .send()
        .await
        .expect("Failed /api/v1/compute");
    assert!(res.status().is_success());

    // Verify metrics store recorded all 3 requests
    assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 3);
    assert_eq!(metrics.success_requests.load(Ordering::Relaxed), 3);

    // Check endpoints map
    {
        let eps = metrics.endpoints.read();
        assert!(eps.contains_key("GET /health"));
        assert!(eps.contains_key("GET /api/v1/users"));
        assert!(eps.contains_key("GET /api/v1/compute"));
    }

    // Teardown
    let _ = shutdown_tx.send(());
    let _ = std::fs::remove_file(test_db);
}

#[test]
fn test_system_monitor_detects_processes() {
    let monitor = ferris_telemetry::system::SystemMonitor::new();
    monitor.refresh();
    let snap = monitor.snapshot();

    println!(
        "Self process: PID={} RAM={:.2}MB CPU={:.1}%",
        snap.self_process.pid, snap.self_process.memory_mb, snap.self_process.cpu_usage
    );
    println!(
        "Go process: PID={} Running={} RAM={:.2}MB CPU={:.1}%",
        snap.go_process.pid,
        snap.go_process.is_running,
        snap.go_process.memory_mb,
        snap.go_process.cpu_usage
    );
    println!(
        "Python process: PID={} Running={} RAM={:.2}MB CPU={:.1}%",
        snap.python_process.pid,
        snap.python_process.is_running,
        snap.python_process.memory_mb,
        snap.python_process.cpu_usage
    );

    assert!(snap.self_process.is_running);
    assert!(snap.self_process.memory_mb > 0.0);
}

#[tokio::test]
async fn test_arena_shootout_against_live_servers() {
    let arena = ferris_telemetry::arena::ArenaManager::new(None);
    let monitor = ferris_telemetry::system::SystemMonitor::new();
    arena.attach_system_monitor(monitor.clone());
    monitor.refresh();
    let snap = monitor.snapshot();

    // Check health of running servers (Axum :3000, Python :8000, Go :8080)
    arena.check_targets_health(&snap).await;

    // Run a fast shootout
    let _ = arena
        .clone()
        .run_shootout(Duration::from_millis(150), 5, 1)
        .await;

    {
        let targets = arena.targets.read();
        for t in targets.iter() {
            if t.is_online {
                println!(
                    "JSON TARGET: {} -> Req/s={:.1} p99={} RAM={:.1}MB",
                    t.name,
                    t.current_rps,
                    ferris_telemetry::metrics::LatencySummary::format_ms(t.p99_us),
                    t.ram_mb
                );
                assert!(t.current_rps > 0.0);
                assert_eq!(t.last_errors, 0);
            }
        }
    }

    arena.toggle_scenario();
    let _ = arena
        .clone()
        .run_shootout(Duration::from_millis(150), 5, 1)
        .await;

    let targets = arena.targets.read();
    for t in targets.iter() {
        if t.is_online {
            println!(
                "POSTGRES TARGET: {} -> Req/s={:.1} p99={} RAM={:.1}MB",
                t.name,
                t.current_rps,
                ferris_telemetry::metrics::LatencySummary::format_ms(t.p99_us),
                t.ram_mb
            );
            assert!(t.current_rps > 0.0);
            assert_eq!(t.last_errors, 0);
        }
    }
}
