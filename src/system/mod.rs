use parking_lot::RwLock;
use std::sync::Arc;
use sysinfo::{
    CpuRefreshKind, MemoryRefreshKind, Pid, ProcessRefreshKind, ProcessesToUpdate, System,
};

#[derive(Debug, Clone, Default)]
pub struct ProcessMetric {
    pub name: String,
    pub pid: u32,
    pub memory_mb: f64,
    pub cpu_usage: f32,
    pub is_running: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SystemSnapshot {
    pub cpu_global: f32,
    pub cpu_cores: Vec<f32>,
    pub memory_used_mb: f64,
    pub memory_total_mb: f64,
    pub memory_percent: f32,
    pub swap_used_mb: f64,
    pub swap_total_mb: f64,
    pub self_process: ProcessMetric,
    pub rust_process: ProcessMetric,
    pub actix_process: ProcessMetric,
    pub go_process: ProcessMetric,
    pub python_process: ProcessMetric,
}

pub struct SystemMonitor {
    sys: RwLock<System>,
    current_snapshot: RwLock<SystemSnapshot>,
    self_pid: Pid,
}

impl SystemMonitor {
    pub fn new() -> Arc<Self> {
        let mut sys = System::new_all();
        sys.refresh_all();
        let self_pid = Pid::from_u32(std::process::id());

        let monitor = Arc::new(Self {
            sys: RwLock::new(sys),
            current_snapshot: RwLock::new(SystemSnapshot::default()),
            self_pid,
        });

        monitor.refresh();
        monitor
    }

    pub fn refresh(&self) {
        let mut sys = self.sys.write();

        // Refresh CPU, Memory, Processes
        sys.refresh_cpu_specifics(CpuRefreshKind::everything());
        sys.refresh_memory_specifics(MemoryRefreshKind::everything());
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::everything(),
        );

        let cpus = sys.cpus();
        let cpu_global = sys.global_cpu_usage();
        let cpu_cores: Vec<f32> = cpus.iter().map(|c| c.cpu_usage()).collect();

        // Memory in MB (sysinfo returns bytes in 0.33)
        let total_mem = sys.total_memory() as f64 / (1024.0 * 1024.0);
        let used_mem = sys.used_memory() as f64 / (1024.0 * 1024.0);
        let mem_pct = if total_mem > 0.0 {
            (used_mem / total_mem * 100.0) as f32
        } else {
            0.0
        };

        let total_swap = sys.total_swap() as f64 / (1024.0 * 1024.0);
        let used_swap = sys.used_swap() as f64 / (1024.0 * 1024.0);

        // Find Ferris TUI process
        let mut self_metric = ProcessMetric {
            name: "FerrisTelemetry TUI".to_string(),
            pid: self.self_pid.as_u32(),
            memory_mb: 0.0,
            cpu_usage: 0.0,
            is_running: true,
        };

        if let Some(proc) = sys.process(self.self_pid) {
            self_metric.memory_mb = proc.memory() as f64 / (1024.0 * 1024.0);
            self_metric.cpu_usage = proc.cpu_usage();
        }

        // Scan for standalone Axum process (:3000)
        let mut rust_metric = ProcessMetric {
            name: "Axum (Rust)".to_string(),
            pid: 0,
            memory_mb: 0.0,
            cpu_usage: 0.0,
            is_running: false,
        };

        // Scan for standalone Actix Web process (:4000)
        let mut actix_metric = ProcessMetric {
            name: "Actix Web (Rust)".to_string(),
            pid: 0,
            memory_mb: 0.0,
            cpu_usage: 0.0,
            is_running: false,
        };

        // Scan for Go process (:8080)
        let mut go_metric = ProcessMetric {
            name: "Fiber (Go)".to_string(),
            pid: 0,
            memory_mb: 0.0,
            cpu_usage: 0.0,
            is_running: false,
        };

        // Scan for Python process (:8000)
        let mut py_metric = ProcessMetric {
            name: "FastAPI (Python)".to_string(),
            pid: 0,
            memory_mb: 0.0,
            cpu_usage: 0.0,
            is_running: false,
        };

        for (pid, proc) in sys.processes() {
            let proc_name = proc.name().to_string_lossy().to_lowercase();
            let exe_name = proc
                .exe()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let cmd = proc
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();

            // 0. Detect Standalone Axum Server (Rust)
            if !rust_metric.is_running
                && (proc_name.contains("axum_server") || exe_name.contains("axum_server"))
                && pid.as_u32() != self.self_pid.as_u32()
            {
                rust_metric.pid = pid.as_u32();
                rust_metric.memory_mb = proc.memory() as f64 / (1024.0 * 1024.0);
                rust_metric.cpu_usage = proc.cpu_usage();
                rust_metric.is_running = true;
            }

            // 0b. Detect Standalone Actix Web Server (Rust)
            if !actix_metric.is_running
                && (proc_name.contains("actix_server") || exe_name.contains("actix_server"))
                && pid.as_u32() != self.self_pid.as_u32()
            {
                actix_metric.pid = pid.as_u32();
                actix_metric.memory_mb = proc.memory() as f64 / (1024.0 * 1024.0);
                actix_metric.cpu_usage = proc.cpu_usage();
                actix_metric.is_running = true;
            }

            // 1. Detect Go Fiber
            if !go_metric.is_running && (proc_name.contains("fiber") || exe_name.contains("fiber"))
            {
                go_metric.pid = pid.as_u32();
                go_metric.memory_mb = proc.memory() as f64 / (1024.0 * 1024.0);
                go_metric.cpu_usage = proc.cpu_usage();
                go_metric.is_running = true;
            }

            // 2. Detect Python FastAPI
            if !py_metric.is_running
                && (proc_name.contains("python") || cmd.contains("python"))
                && (cmd.contains("python_fastapi")
                    || cmd.contains("fastapi")
                    || cmd.contains("main.py")
                    || cmd.contains("uvicorn")
                    || cmd.contains("8000"))
            {
                py_metric.pid = pid.as_u32();
                py_metric.memory_mb = proc.memory() as f64 / (1024.0 * 1024.0);
                py_metric.cpu_usage = proc.cpu_usage();
                py_metric.is_running = true;
            }
        }

        let snapshot = SystemSnapshot {
            cpu_global,
            cpu_cores,
            memory_used_mb: used_mem,
            memory_total_mb: total_mem,
            memory_percent: mem_pct,
            swap_used_mb: used_swap,
            swap_total_mb: total_swap,
            self_process: self_metric,
            rust_process: rust_metric,
            actix_process: actix_metric,
            go_process: go_metric,
            python_process: py_metric,
        };

        *self.current_snapshot.write() = snapshot;
    }

    pub fn snapshot(&self) -> SystemSnapshot {
        self.current_snapshot.read().clone()
    }

    pub fn process_metric(&self, pid: u32, name: &str) -> Option<ProcessMetric> {
        let sys = self.sys.read();
        let process = sys.process(Pid::from_u32(pid))?;
        Some(ProcessMetric {
            name: name.to_string(),
            pid,
            memory_mb: process.memory() as f64 / (1024.0 * 1024.0),
            cpu_usage: process.cpu_usage(),
            is_running: true,
        })
    }
}
