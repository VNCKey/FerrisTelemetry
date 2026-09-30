use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub role: String,
    pub created_at: Option<String>,
}

pub async fn init_db(db_path: &str) -> Result<SqlitePool> {
    let connection_string = format!("sqlite:{}?mode=rwc", db_path);
    let options = SqliteConnectOptions::from_str(&connection_string)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(20)
        .connect_with(options)
        .await?;

    // Create tables
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            email TEXT NOT NULL UNIQUE,
            role TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS benchmarks_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            target_name TEXT NOT NULL,
            req_per_sec REAL NOT NULL,
            p99_latency_ms REAL NOT NULL,
            ram_mb REAL NOT NULL,
            recorded_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS benchmark_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            target_name TEXT NOT NULL,
            scenario TEXT NOT NULL,
            req_per_sec REAL NOT NULL,
            rps_variation_pct REAL NOT NULL,
            p50_latency_ms REAL NOT NULL,
            p95_latency_ms REAL NOT NULL,
            p99_latency_ms REAL NOT NULL,
            successful_requests INTEGER NOT NULL,
            failed_requests INTEGER NOT NULL,
            rounds INTEGER NOT NULL,
            duration_seconds REAL NOT NULL,
            peak_ram_mb REAL NOT NULL,
            recorded_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        "#,
    )
    .execute(&pool)
    .await?;

    // Check if users exist, otherwise seed
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await?;

    if count.0 == 0 {
        let sample_users = [
            ("ferris_crab", "ferris@rust-lang.org", "admin"),
            ("tokio_runner", "tokio@async.rs", "core"),
            ("axum_master", "axum@tower.rs", "developer"),
            ("ratatui_artist", "ratatui@tui.rs", "designer"),
            ("crossterm_io", "terminal@crossterm.rs", "engineer"),
            ("sqlx_query", "sqlx@database.rs", "dba"),
            ("sysinfo_agent", "monitor@kernel.org", "devops"),
            ("fiber_rival", "fiber@gofiber.io", "guest"),
            ("fastapi_star", "fastapi@tiangolo.com", "guest"),
            ("spring_heavy", "spring@jvm.org", "guest"),
        ];

        for (username, email, role) in sample_users {
            let _ =
                sqlx::query("INSERT OR IGNORE INTO users (username, email, role) VALUES (?, ?, ?)")
                    .bind(username)
                    .bind(email)
                    .bind(role)
                    .execute(&pool)
                    .await;
        }
    }

    Ok(pool)
}

pub async fn get_users(pool: &SqlitePool) -> Result<Vec<User>> {
    let users = sqlx::query_as::<_, User>(
        "SELECT id, username, email, role, created_at FROM users LIMIT 10",
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

#[allow(dead_code)]
pub async fn save_benchmark_result(
    pool: &SqlitePool,
    target: &str,
    rps: f64,
    p99_ms: f64,
    ram_mb: f64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO benchmarks_history (target_name, req_per_sec, p99_latency_ms, ram_mb) VALUES (?, ?, ?, ?)"
    )
    .bind(target)
    .bind(rps)
    .bind(p99_ms)
    .bind(ram_mb)
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn save_benchmark_run(
    pool: &SqlitePool,
    target: &str,
    scenario: &str,
    rps: f64,
    rps_variation_pct: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    successful_requests: u64,
    failed_requests: u64,
    rounds: usize,
    duration_seconds: f64,
    peak_ram_mb: f64,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO benchmark_runs (
            target_name, scenario, req_per_sec, rps_variation_pct,
            p50_latency_ms, p95_latency_ms, p99_latency_ms,
            successful_requests, failed_requests, rounds,
            duration_seconds, peak_ram_mb
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(target)
    .bind(scenario)
    .bind(rps)
    .bind(rps_variation_pct)
    .bind(p50_ms)
    .bind(p95_ms)
    .bind(p99_ms)
    .bind(successful_requests as i64)
    .bind(failed_requests as i64)
    .bind(rounds as i64)
    .bind(duration_seconds)
    .bind(peak_ram_mb)
    .execute(pool)
    .await?;
    Ok(())
}
