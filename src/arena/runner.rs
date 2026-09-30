use anyhow::Result;
use parking_lot::RwLock;
use ratatui::style::Color;
use reqwest::Client;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::target::FrameworkTarget;
use crate::db;
use crate::metrics::histogram::LatencySummary;
use crate::system::{SystemMonitor, SystemSnapshot};

use std::process::{Child, Command, Stdio};
use tokio_postgres::NoTls;

fn binary_is_current(binary: &str, sources: &[&str]) -> bool {
    let Ok(binary_time) = std::fs::metadata(binary).and_then(|meta| meta.modified()) else {
        return false;
    };
    sources.iter().all(|source| {
        std::fs::metadata(source)
            .and_then(|meta| meta.modified())
            .map(|source_time| source_time <= binary_time)
            .unwrap_or(false)
    })
}

fn stale_binary_error(binary: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!(
            "{} no existe o está desactualizado; ejecuta ./scripts/build-benchmarks.sh",
            binary
        ),
    )
}

fn port_available(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

fn port_busy_error(port: u16) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::AddrInUse,
        format!("el puerto {} ya está ocupado", port),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BenchmarkScenario {
    Json,
    Postgres,
    PostgresSingle,
    PostgresMulti,
    PostgresWrite,
}

impl BenchmarkScenario {
    pub fn label(self) -> &'static str {
        match self {
            Self::Json => "HTTP/JSON (sin DB)",
            Self::Postgres => "PostgreSQL (lista 10)",
            Self::PostgresSingle => "PostgreSQL (1 fila, IDs variables)",
            Self::PostgresMulti => "PostgreSQL (multi-lectura, 10 IDs)",
            Self::PostgresWrite => "PostgreSQL (escritura upsert)",
        }
    }

    fn request_url(self, base_url: &str, sequence: u64) -> String {
        let id = benchmark_id(sequence);
        match self {
            Self::Json => format!("{base_url}/api/v1/plain"),
            Self::Postgres => format!("{base_url}/api/v1/users"),
            Self::PostgresSingle => format!("{base_url}/api/v1/user?id={id}"),
            Self::PostgresMulti => {
                let first_id = id - 1;
                let ids = (0..10)
                    .map(|offset| (first_id + offset as u64) % 100_000 + 1)
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{base_url}/api/v1/queries?ids={ids}")
            }
            Self::PostgresWrite => format!("{base_url}/api/v1/write?id={id}"),
        }
    }

    fn expected_source(self) -> &'static [u8] {
        match self {
            Self::Json => b"\"source\":\"memory\"",
            Self::Postgres | Self::PostgresSingle | Self::PostgresMulti | Self::PostgresWrite => {
                b"\"source\":\"postgres\""
            }
        }
    }

    fn payload_is_valid(self, body: &[u8]) -> bool {
        contains_bytes(body, self.expected_source())
            && match self {
                Self::Json => {
                    contains_bytes(body, b"\"message\":\"FerrisTelemetry benchmark payload\"")
                }
                Self::Postgres => contains_bytes(body, b"\"count\":10"),
                Self::PostgresSingle => contains_bytes(body, b"\"user\":{"),
                Self::PostgresMulti => contains_bytes(body, b"\"count\":10"),
                Self::PostgresWrite => contains_bytes(body, b"\"written\":true"),
            }
    }

    fn toggled(self) -> Self {
        match self {
            Self::Json => Self::Postgres,
            Self::Postgres => Self::PostgresSingle,
            Self::PostgresSingle => Self::PostgresMulti,
            Self::PostgresMulti => Self::PostgresWrite,
            Self::PostgresWrite => Self::Json,
        }
    }
}

#[derive(Default)]
struct RoundResult {
    successful: u64,
    errors: u64,
    latencies_us: Vec<u64>,
    elapsed: Duration,
    peak_ram_mb: f64,
}

#[derive(Default)]
struct TargetAggregate {
    rps_by_round: Vec<f64>,
    latencies_us: Vec<u64>,
    successful: u64,
    errors: u64,
    peak_ram_mb: f64,
}

pub struct ArenaManager {
    pub targets: RwLock<Vec<FrameworkTarget>>,
    pub is_running_benchmark: RwLock<bool>,
    pub benchmark_progress: RwLock<f32>,
    pub execution_log: RwLock<Vec<String>>,
    pub current_scenario: RwLock<BenchmarkScenario>,
    client: Client,
    db_pool: Option<SqlitePool>,
    system_monitor: RwLock<Option<Arc<SystemMonitor>>>,
    companion_children: RwLock<Vec<CompanionProcess>>,
}

struct CompanionProcess {
    target_id: &'static str,
    child: Child,
}

impl ArenaManager {
    pub fn new(db_pool: Option<SqlitePool>) -> Arc<Self> {
        let targets = vec![
            FrameworkTarget::new("rust_axum", "Rust (Axum)", 3000, Color::Cyan),
            FrameworkTarget::new("rust_actix", "Rust (Actix)", 4000, Color::Magenta),
            FrameworkTarget::new("go", "Go (Fiber)", 8080, Color::LightBlue),
            FrameworkTarget::new("python", "Python (FastAPI)", 8000, Color::Yellow),
        ];

        let client = Client::builder()
            .timeout(Duration::from_secs(3))
            .pool_max_idle_per_host(50)
            .tcp_nodelay(true)
            .http1_only()
            .build()
            .unwrap_or_default();

        Arc::new(Self {
            targets: RwLock::new(targets),
            is_running_benchmark: RwLock::new(false),
            benchmark_progress: RwLock::new(0.0),
            execution_log: RwLock::new(vec![
                "FerrisTelemetry Framework Arena inicializada.".to_string(),
                "INFO: Presiona [a] para arrancar/detener Axum, Actix, Fiber y FastAPI."
                    .to_string(),
                "INFO: Presiona [b] para ejecutar un benchmark contra los servidores activos."
                    .to_string(),
                "INFO: Presiona [v] para probar la matriz de concurrencia 8..256.".to_string(),
            ]),
            current_scenario: RwLock::new(BenchmarkScenario::Json),
            client,
            db_pool,
            system_monitor: RwLock::new(None),
            companion_children: RwLock::new(Vec::new()),
        })
    }

    pub fn attach_system_monitor(&self, monitor: Arc<SystemMonitor>) {
        *self.system_monitor.write() = Some(monitor);
    }

    pub fn toggle_scenario(&self) -> BenchmarkScenario {
        let mut scenario = self.current_scenario.write();
        *scenario = scenario.toggled();
        self.log(format!(
            "INFO: Escenario seleccionado: {}",
            scenario.label()
        ));
        *scenario
    }

    /// Toggle background companion servers (Axum, Actix, Fiber & FastAPI) within the same terminal
    pub fn toggle_companions(&self) -> bool {
        let mut children = self.companion_children.write();
        if !children.is_empty() {
            for mut process in children.drain(..) {
                let _ = process.child.kill();
                let _ = process.child.wait();
            }

            self.log("OK: Servidores de Axum, Actix, Fiber y FastAPI detenidos.".to_string());
            false
        } else {
            // 1. Spawn Rust Axum (:3000)
            let axum_bin = "target/release/axum_server";
            let axum_res = if !port_available(3000) {
                Err(port_busy_error(3000))
            } else if binary_is_current(
                axum_bin,
                &["benchmarks/rust_axum/main.rs", "Cargo.toml", "Cargo.lock"],
            ) {
                Command::new(format!("./{}", axum_bin))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            } else {
                Err(stale_binary_error(axum_bin))
            };

            match axum_res {
                Ok(child) => {
                    children.push(CompanionProcess {
                        target_id: "rust_axum",
                        child,
                    });
                    self.log(
                        "OK: Servidor Axum (Rust :3000) lanzado en segundo plano.".to_string(),
                    );
                }
                Err(e) => {
                    self.log(format!("WARN: No se pudo lanzar Axum: {}", e));
                }
            }

            // 1b. Spawn Rust Actix Web (:4000)
            let actix_bin = "target/release/actix_server";
            let actix_res = if !port_available(4000) {
                Err(port_busy_error(4000))
            } else if binary_is_current(
                actix_bin,
                &["benchmarks/rust_actix/main.rs", "Cargo.toml", "Cargo.lock"],
            ) {
                Command::new(format!("./{}", actix_bin))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            } else {
                Err(stale_binary_error(actix_bin))
            };

            match actix_res {
                Ok(child) => {
                    children.push(CompanionProcess {
                        target_id: "rust_actix",
                        child,
                    });
                    self.log(
                        "OK: Servidor Actix Web (Rust :4000) lanzado en segundo plano.".to_string(),
                    );
                }
                Err(e) => {
                    self.log(format!("WARN: No se pudo lanzar Actix Web: {}", e));
                }
            }

            // 2. Spawn Go Fiber (:8080)
            let go_bin = "benchmarks/go_fiber/fiber_server";
            let go_res = if !port_available(8080) {
                Err(port_busy_error(8080))
            } else if binary_is_current(
                go_bin,
                &[
                    "benchmarks/go_fiber/main.go",
                    "benchmarks/go_fiber/go.mod",
                    "benchmarks/go_fiber/go.sum",
                ],
            ) {
                Command::new(format!("./{}", go_bin))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            } else {
                Err(stale_binary_error(go_bin))
            };

            match go_res {
                Ok(child) => {
                    children.push(CompanionProcess {
                        target_id: "go",
                        child,
                    });
                    self.log("OK: Servidor Fiber (Go :8080) lanzado en segundo plano.".to_string());
                }
                Err(e) => {
                    self.log(format!("WARN: No se pudo lanzar Go Fiber: {}", e));
                }
            }

            // 3. Spawn Python FastAPI (:8000)
            let python = "benchmarks/python_fastapi/.venv/bin/python";
            let py_res = if std::path::Path::new(python).exists() {
                if !port_available(8000) {
                    Err(port_busy_error(8000))
                } else {
                    Command::new(python)
                        .arg("benchmarks/python_fastapi/main.py")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                }
            } else {
                Err(stale_binary_error(python))
            };

            match py_res {
                Ok(child) => {
                    children.push(CompanionProcess {
                        target_id: "python",
                        child,
                    });
                    self.log(
                        "OK: Servidor FastAPI (Python :8000) lanzado en segundo plano.".to_string(),
                    );
                }
                Err(e) => {
                    self.log(format!(
                        "WARN: No se pudo lanzar Python: {}. Ejecuta ./scripts/setup-python.sh",
                        e
                    ));
                }
            }

            self.log("RUN: Servidores iniciados. Detectando targets...".to_string());
            true
        }
    }

    pub fn stop_all_companions(&self) {
        let mut children = self.companion_children.write();
        for mut process in children.drain(..) {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }

    fn companion_pid(&self, target_id: &str) -> Option<u32> {
        self.companion_children
            .read()
            .iter()
            .find(|process| process.target_id == target_id)
            .map(|process| process.child.id())
    }

    pub fn log(&self, message: String) {
        let mut log = self.execution_log.write();
        if log.len() > 50 {
            log.remove(0);
        }
        log.push(message);
    }

    /// Healthcheck routine called periodically to check which frameworks are online
    pub async fn check_targets_health(&self, sys_snap: &SystemSnapshot) {
        let targets_snapshot = { self.targets.read().clone() };

        for (idx, target) in targets_snapshot.iter().enumerate() {
            let url = format!("{}/health", target.base_url);
            let check_res = self
                .client
                .get(&url)
                .timeout(Duration::from_millis(600))
                .send()
                .await;

            let mut targets = self.targets.write();
            match check_res {
                Ok(resp) => {
                    let online = resp.status().is_success();
                    targets[idx].is_online = online;
                    targets[idx].last_status_code = resp.status().as_u16();
                }
                Err(_) => {
                    targets[idx].is_online = false;
                    targets[idx].last_status_code = 0;
                }
            }

            // Sync RAM and CPU from system monitor
            if target.id == "rust_axum" || target.id == "rust" {
                if sys_snap.rust_process.is_running {
                    targets[idx].ram_mb = sys_snap.rust_process.memory_mb;
                    targets[idx].cpu_pct = sys_snap.rust_process.cpu_usage;
                } else {
                    targets[idx].ram_mb = sys_snap.self_process.memory_mb;
                    targets[idx].cpu_pct = sys_snap.self_process.cpu_usage;
                }
            } else if target.id == "rust_actix" {
                targets[idx].ram_mb = sys_snap.actix_process.memory_mb;
                targets[idx].cpu_pct = sys_snap.actix_process.cpu_usage;
            } else if target.id == "go" {
                targets[idx].ram_mb = sys_snap.go_process.memory_mb;
                targets[idx].cpu_pct = sys_snap.go_process.cpu_usage;
            } else if target.id == "python" {
                targets[idx].ram_mb = sys_snap.python_process.memory_mb;
                targets[idx].cpu_pct = sys_snap.python_process.cpu_usage;
            }

            if let (Some(pid), Some(monitor)) = (
                self.companion_pid(&target.id),
                self.system_monitor.read().clone(),
            ) {
                if let Some(metric) = monitor.process_metric(pid, &target.name) {
                    targets[idx].ram_mb = metric.memory_mb;
                    targets[idx].cpu_pct = metric.cpu_usage;
                }
            }
        }
    }

    /// Runs a duration-based, multi-round comparison. Only successful responses
    /// with the expected payload are counted as throughput.
    pub async fn run_shootout(
        self: Arc<Self>,
        round_duration: Duration,
        concurrency: usize,
        rounds: usize,
    ) -> Result<()> {
        if *self.is_running_benchmark.read() {
            return Ok(());
        }

        let concurrency = concurrency.max(1);
        let rounds = rounds.max(1);
        let round_duration = round_duration.max(Duration::from_millis(100));
        let scenario = *self.current_scenario.read();
        let stored_scenario = format!("{} | concurrency={concurrency}", scenario.label());

        *self.is_running_benchmark.write() = true;
        *self.benchmark_progress.write() = 0.01;

        self.log(format!(
            "BENCH: {}: {} rondas x {:.1}s por target (concurrencia {}, pool DB fijo=20, respuesta validada)...",
            scenario.label(),
            rounds,
            round_duration.as_secs_f64(),
            concurrency
        ));

        let targets_to_test: Vec<(usize, String, String)> = {
            let targets = self.targets.read();
            targets
                .iter()
                .enumerate()
                .filter(|(_, t)| t.is_online)
                .map(|(idx, t)| (idx, t.id.clone(), t.base_url.clone()))
                .collect()
        };

        if targets_to_test.is_empty() {
            self.log(
                "WARN: Ningún servidor detectado online. Revisa los binarios y PostgreSQL."
                    .to_string(),
            );
            *self.is_running_benchmark.write() = false;
            *self.benchmark_progress.write() = 0.0;
            return Ok(());
        }

        let num_targets = targets_to_test.len();
        let total_steps = num_targets * rounds;
        let mut completed_steps = 0usize;
        let mut aggregates: HashMap<usize, TargetAggregate> = HashMap::new();

        for round in 0..rounds {
            // A complete rotation gives every target each ordinal position once
            // when the number of rounds matches the number of targets.
            let mut ordered = targets_to_test.clone();
            let order_len = ordered.len();
            ordered.rotate_left(round % order_len);

            for (target_idx, target_id, base_url) in ordered {
                let (target_name, baseline_ram_mb) = {
                    let targets = self.targets.read();
                    (targets[target_idx].name.clone(), targets[target_idx].ram_mb)
                };
                let endpoint = scenario.request_url(&base_url, 0);

                self.log(format!(
                    "RUN: Ronda {}/{}: {}...",
                    round + 1,
                    rounds,
                    target_name
                ));

                if !self.validate_endpoint(&endpoint, scenario).await {
                    self.log(format!(
                        "WARN: {} omitido: respuesta inválida para {}.",
                        target_name,
                        scenario.label()
                    ));
                    let aggregate = aggregates.entry(target_idx).or_default();
                    aggregate.errors += 1;
                    completed_steps += 1;
                    *self.benchmark_progress.write() = completed_steps as f32 / total_steps as f32;
                    continue;
                }

                self.warm_up(&base_url, scenario, concurrency).await;
                let result = self
                    .run_target_round(base_url, target_id, scenario, round_duration, concurrency)
                    .await;
                let peak_ram_mb = result.peak_ram_mb;
                let rps = if result.elapsed.as_secs_f64() > 0.0 {
                    result.successful as f64 / result.elapsed.as_secs_f64()
                } else {
                    0.0
                };

                let aggregate = aggregates.entry(target_idx).or_default();
                aggregate.rps_by_round.push(rps);
                aggregate.latencies_us.extend(result.latencies_us);
                aggregate.successful += result.successful;
                aggregate.errors += result.errors;
                aggregate.peak_ram_mb = aggregate.peak_ram_mb.max(peak_ram_mb).max(baseline_ram_mb);

                completed_steps += 1;
                *self.benchmark_progress.write() = completed_steps as f32 / total_steps as f32;
            }
        }

        for (target_idx, mut aggregate) in aggregates {
            let target_name = self.targets.read()[target_idx].name.clone();
            let median_rps = median_f64(&mut aggregate.rps_by_round);
            let variation = coefficient_of_variation_pct(&aggregate.rps_by_round);
            let summary = LatencySummary::from_samples(aggregate.latencies_us);

            {
                let mut targets = self.targets.write();
                let target = &mut targets[target_idx];
                target.current_rps = median_rps;
                target.rps_variation_pct = variation;
                target.p50_us = summary.p50_us;
                target.p95_us = summary.p95_us;
                target.p99_us = summary.p99_us;
                target.min_us = summary.min_us;
                target.max_us = summary.max_us;
                target.last_successes = aggregate.successful;
                target.last_errors = aggregate.errors;
                target.benchmark_rounds = aggregate.rps_by_round.len();
                target.total_tested += aggregate.successful + aggregate.errors;
                target.total_errors += aggregate.errors;
                target.ram_mb = target.ram_mb.max(aggregate.peak_ram_mb);
            }

            if let Some(ref pool) = self.db_pool {
                let _ = db::save_benchmark_run(
                    pool,
                    &target_name,
                    &stored_scenario,
                    median_rps,
                    variation,
                    summary.p50_us as f64 / 1000.0,
                    summary.p95_us as f64 / 1000.0,
                    summary.p99_us as f64 / 1000.0,
                    aggregate.successful,
                    aggregate.errors,
                    aggregate.rps_by_round.len(),
                    round_duration.as_secs_f64(),
                    aggregate.peak_ram_mb,
                )
                .await;
            }

            self.log(format!(
                "OK: {}: {:.0} req/s (variación {:.1}%) | p99 {} | ok {} err {}",
                target_name,
                median_rps,
                variation,
                LatencySummary::format_ms(summary.p99_us),
                aggregate.successful,
                aggregate.errors
            ));
        }

        if let Some(stats) = sample_postgres_stats().await {
            let cpu = postgres_container_cpu_pct()
                .map(|value| format!("{value:.1}%"))
                .unwrap_or_else(|| "n/d".to_string());
            self.log(format!(
                "DB: PostgreSQL CPU {} | conexiones activas {}/{} | cache hit {:.2}%",
                cpu, stats.active_connections, stats.total_connections, stats.cache_hit_pct
            ));
        } else {
            self.log("WARN: No se pudieron leer las métricas de PostgreSQL.".to_string());
        }

        self.log("OK: benchmark completado; historial guardado en SQLite.".to_string());
        *self.is_running_benchmark.write() = false;
        *self.benchmark_progress.write() = 1.0;

        Ok(())
    }

    /// Executes the selected scenario at every requested concurrency level.
    /// Each level is persisted as an independent shootout so the history can
    /// be compared later without mixing samples from different loads.
    pub async fn run_saturation_matrix(
        self: Arc<Self>,
        round_duration: Duration,
        levels: &[usize],
        rounds: usize,
    ) -> Result<()> {
        let levels: Vec<usize> = levels.iter().copied().filter(|level| *level > 0).collect();
        if levels.is_empty() {
            return Ok(());
        }
        let scenario = *self.current_scenario.read();
        self.log(format!(
            "MATRIX: {} | concurrencias {:?} | {} ronda(s) por nivel",
            scenario.label(),
            levels,
            rounds
        ));
        for (index, level) in levels.iter().enumerate() {
            self.log(format!(
                "MATRIX: nivel {}/{}: concurrencia {}",
                index + 1,
                levels.len(),
                level
            ));
            self.clone()
                .run_shootout(round_duration, *level, rounds)
                .await?;
        }
        self.log(
            "OK: matriz de concurrencia completada; revisa el historial por nivel.".to_string(),
        );
        Ok(())
    }

    async fn validate_endpoint(&self, endpoint: &str, scenario: BenchmarkScenario) -> bool {
        let request = if scenario == BenchmarkScenario::PostgresWrite {
            self.client.post(endpoint)
        } else {
            self.client.get(endpoint)
        };
        match request.send().await {
            Ok(response) if response.status().is_success() => response
                .bytes()
                .await
                .map(|body| scenario.payload_is_valid(&body))
                .unwrap_or(false),
            _ => false,
        }
    }

    async fn warm_up(&self, base_url: &str, scenario: BenchmarkScenario, concurrency: usize) {
        let mut handles = Vec::with_capacity(concurrency);
        for worker_index in 0..concurrency {
            let client = self.client.clone();
            let base_url = base_url.to_string();
            handles.push(tokio::spawn(async move {
                for request_index in 0..10u64 {
                    let sequence = worker_index as u64 * 10 + request_index;
                    let endpoint = scenario.request_url(&base_url, sequence);
                    let request = if scenario == BenchmarkScenario::PostgresWrite {
                        client.post(&endpoint)
                    } else {
                        client.get(&endpoint)
                    };
                    if let Ok(response) = request.send().await {
                        if let Ok(body) = response.bytes().await {
                            let _ = scenario.payload_is_valid(&body);
                        }
                    }
                }
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }
    }

    async fn run_target_round(
        &self,
        base_url: String,
        target_id: String,
        scenario: BenchmarkScenario,
        duration: Duration,
        concurrency: usize,
    ) -> RoundResult {
        let start = Instant::now();
        let deadline = start + duration;
        let stop = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::with_capacity(concurrency);

        let resource_sampler = self.system_monitor.read().clone().map(|monitor| {
            let stop = stop.clone();
            let companion_pid = self.companion_pid(&target_id);
            tokio::spawn(async move {
                let mut peak_ram_mb = 0.0_f64;
                while !stop.load(Ordering::Relaxed) {
                    monitor.refresh();
                    let snapshot = monitor.snapshot();
                    let ram_mb = companion_pid
                        .and_then(|pid| monitor.process_metric(pid, &target_id))
                        .map(|metric| metric.memory_mb)
                        .unwrap_or_else(|| target_ram_mb(&snapshot, &target_id));
                    peak_ram_mb = peak_ram_mb.max(ram_mb);
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
                peak_ram_mb
            })
        });

        for worker_index in 0..concurrency {
            let client = self.client.clone();
            let base_url = base_url.clone();
            let stop = stop.clone();
            handles.push(tokio::spawn(async move {
                let mut result = RoundResult::default();
                let mut sequence = worker_index as u64;
                while Instant::now() < deadline && !stop.load(Ordering::Relaxed) {
                    let request_start = Instant::now();
                    let endpoint = scenario.request_url(&base_url, sequence);
                    sequence += concurrency as u64;
                    let request = if scenario == BenchmarkScenario::PostgresWrite {
                        client.post(&endpoint)
                    } else {
                        client.get(&endpoint)
                    };
                    let response = request.send().await;
                    let valid = match response {
                        Ok(response) if response.status().is_success() => response
                            .bytes()
                            .await
                            .map(|body| scenario.payload_is_valid(&body))
                            .unwrap_or(false),
                        _ => false,
                    };
                    let latency_us = request_start.elapsed().as_micros() as u64;
                    if valid {
                        result.successful += 1;
                        result.latencies_us.push(latency_us);
                    } else {
                        result.errors += 1;
                    }
                }
                result
            }));
        }

        tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
        stop.store(true, Ordering::Relaxed);

        let mut combined = RoundResult::default();
        for handle in handles {
            match handle.await {
                Ok(result) => {
                    combined.successful += result.successful;
                    combined.errors += result.errors;
                    combined.latencies_us.extend(result.latencies_us);
                }
                Err(_) => combined.errors += 1,
            }
        }
        combined.elapsed = start.elapsed();
        if let Some(sampler) = resource_sampler {
            combined.peak_ram_mb = sampler.await.unwrap_or_default();
        }
        combined
    }
}

#[derive(Debug, Default)]
struct PostgresStats {
    active_connections: i64,
    total_connections: i64,
    cache_hit_pct: f64,
}

async fn sample_postgres_stats() -> Option<PostgresStats> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://ferris:ferris@127.0.0.1:5432/ferris_bench".to_string());
    let (client, connection) = tokio_postgres::connect(&database_url, NoTls).await.ok()?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let row = client
        .query_one(
            "SELECT
                (SELECT count(*) FROM pg_stat_activity
                    WHERE datname = current_database() AND state = 'active')::bigint,
                (SELECT count(*) FROM pg_stat_activity
                    WHERE datname = current_database())::bigint,
                COALESCE((SELECT blks_hit::double precision * 100.0 /
                    NULLIF(blks_hit + blks_read, 0)
                    FROM pg_stat_database WHERE datname = current_database()), 100.0)",
            &[],
        )
        .await
        .ok()?;
    Some(PostgresStats {
        active_connections: row.get(0),
        total_connections: row.get(1),
        cache_hit_pct: row.get(2),
    })
}

fn postgres_container_cpu_pct() -> Option<f64> {
    let id = Command::new("docker")
        .args(["compose", "ps", "-q", "postgres"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let output = Command::new("docker")
        .args(["stats", "--no-stream", "--format", "{{.CPUPerc}}", &id])
        .output()
        .ok()
        .filter(|output| output.status.success())?;
    let value = String::from_utf8(output.stdout).ok()?;
    value.trim().trim_end_matches('%').parse().ok()
}

fn target_ram_mb(snapshot: &SystemSnapshot, target_id: &str) -> f64 {
    match target_id {
        "rust_axum" | "rust" => snapshot.rust_process.memory_mb,
        "rust_actix" => snapshot.actix_process.memory_mb,
        "go" => snapshot.go_process.memory_mb,
        "python" => snapshot.python_process.memory_mb,
        _ => 0.0,
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Fast deterministic pseudo-random IDs keep every request on the indexed
/// primary-key path without adding an RNG lock or allocation to the server.
fn benchmark_id(sequence: u64) -> u64 {
    let mut value = sequence.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    value % 100_000 + 1
}

fn median_f64(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn coefficient_of_variation_pct(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean <= f64::EPSILON {
        return 0.0;
    }
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt() / mean * 100.0
}

#[cfg(test)]
mod tests {
    use super::{
        benchmark_id, coefficient_of_variation_pct, contains_bytes, median_f64, BenchmarkScenario,
    };

    #[test]
    fn median_handles_odd_even_and_empty_samples() {
        assert_eq!(median_f64(&mut []), 0.0);
        assert_eq!(median_f64(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median_f64(&mut [4.0, 1.0, 3.0, 2.0]), 2.5);
    }

    #[test]
    fn variation_is_zero_for_stable_or_single_rounds() {
        assert_eq!(coefficient_of_variation_pct(&[100.0]), 0.0);
        assert_eq!(coefficient_of_variation_pct(&[100.0, 100.0, 100.0]), 0.0);
        assert!(coefficient_of_variation_pct(&[80.0, 100.0, 120.0]) > 10.0);
    }

    #[test]
    fn payload_validation_requires_expected_source() {
        let body = br#"{"source":"postgres","count":10}"#;
        assert!(contains_bytes(body, b"\"source\":\"postgres\""));
        assert!(!contains_bytes(body, b"\"source\":\"memory\""));
        assert!(BenchmarkScenario::Postgres.payload_is_valid(body));
        assert!(!BenchmarkScenario::Json.payload_is_valid(body));
    }

    #[test]
    fn generated_ids_stay_inside_seeded_dataset() {
        for sequence in [0, 1, 8, 256, 100_000, u64::MAX] {
            assert!((1..=100_000).contains(&benchmark_id(sequence)));
        }
    }

    #[test]
    fn scenario_urls_change_ids_between_requests() {
        let first = BenchmarkScenario::PostgresSingle.request_url("http://127.0.0.1:3000", 1);
        let second = BenchmarkScenario::PostgresSingle.request_url("http://127.0.0.1:3000", 2);
        assert_ne!(first, second);
        assert!(BenchmarkScenario::PostgresMulti
            .request_url("http://127.0.0.1:3000", 1)
            .contains("ids="));
    }
}
