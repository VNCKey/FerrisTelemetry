use ratatui::style::Color;

#[derive(Debug, Clone)]
pub struct FrameworkTarget {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub base_url: String,
    pub color: Color,
    pub is_online: bool,
    pub current_rps: f64,
    pub rps_variation_pct: f64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub min_us: u64,
    pub max_us: u64,
    pub total_tested: u64,
    pub total_errors: u64,
    pub last_successes: u64,
    pub last_errors: u64,
    pub benchmark_rounds: usize,
    pub ram_mb: f64,
    pub cpu_pct: f32,
    pub last_status_code: u16,
}

impl FrameworkTarget {
    pub fn new(id: &str, name: &str, port: u16, color: Color) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            port,
            base_url: format!("http://127.0.0.1:{}", port),
            color,
            is_online: false,
            current_rps: 0.0,
            rps_variation_pct: 0.0,
            p50_us: 0,
            p95_us: 0,
            p99_us: 0,
            min_us: 0,
            max_us: 0,
            total_tested: 0,
            total_errors: 0,
            last_successes: 0,
            last_errors: 0,
            benchmark_rounds: 0,
            ram_mb: 0.0,
            cpu_pct: 0.0,
            last_status_code: 0,
        }
    }

    /// Efficiency metric: Requests per second per MB of RAM consumed
    pub fn efficiency_score(&self) -> f64 {
        if self.ram_mb <= 0.1 || !self.is_online {
            0.0
        } else {
            self.current_rps / self.ram_mb
        }
    }
}
