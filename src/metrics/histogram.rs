/// Fast in-memory latency percentile calculator for zero-allocation telemetry
#[derive(Debug, Clone, Copy, Default)]
pub struct LatencySummary {
    pub min_us: u64,
    pub max_us: u64,
    pub avg_us: u64,
    pub p50_us: u64,
    pub p90_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub sample_count: usize,
}

impl LatencySummary {
    pub fn from_samples(mut samples: Vec<u64>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }

        samples.sort_unstable();
        let count = samples.len();
        let sum: u64 = samples.iter().sum();
        let avg_us = sum / (count as u64);

        let percentile = |pct: f64| -> u64 {
            if count == 0 {
                return 0;
            }
            let idx = ((count as f64) * (pct / 100.0)).ceil() as usize;
            let idx = if idx > 0 { idx - 1 } else { 0 };
            samples[idx.min(count - 1)]
        };

        Self {
            min_us: samples[0],
            max_us: samples[count - 1],
            avg_us,
            p50_us: percentile(50.0),
            p90_us: percentile(90.0),
            p95_us: percentile(95.0),
            p99_us: percentile(99.0),
            sample_count: count,
        }
    }

    pub fn format_ms(us: u64) -> String {
        if us < 1000 {
            format!("{} µs", us)
        } else if us < 1_000_000 {
            format!("{:.2} ms", us as f64 / 1000.0)
        } else {
            format!("{:.2} s", us as f64 / 1_000_000.0)
        }
    }
}
