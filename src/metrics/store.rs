use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use super::histogram::LatencySummary;

const MAX_HISTORY_POINTS: usize = 60;
const MAX_SAMPLES_PER_WINDOW: usize = 2000;

#[derive(Debug)]
pub struct EndpointStat {
    pub path: String,
    pub method: String,
    pub count: AtomicU64,
    pub errors: AtomicU64,
    pub avg_latency_us: AtomicU64,
    pub p99_latency_us: AtomicU64,
    pub min_latency_us: AtomicU64,
    pub max_latency_us: AtomicU64,
    samples: RwLock<Vec<u64>>,
}

impl EndpointStat {
    pub fn new(path: String, method: String) -> Self {
        Self {
            path,
            method,
            count: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            avg_latency_us: AtomicU64::new(0),
            p99_latency_us: AtomicU64::new(0),
            min_latency_us: AtomicU64::new(u64::MAX),
            max_latency_us: AtomicU64::new(0),
            samples: RwLock::new(Vec::with_capacity(200)),
        }
    }

    pub fn record(&self, latency_us: u64, is_error: bool) {
        self.count.fetch_add(1, Ordering::Relaxed);
        if is_error {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }

        // Lock-free min/max updates
        let mut current_min = self.min_latency_us.load(Ordering::Relaxed);
        while latency_us < current_min {
            match self.min_latency_us.compare_exchange_weak(
                current_min,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_min = actual,
            }
        }

        let mut current_max = self.max_latency_us.load(Ordering::Relaxed);
        while latency_us > current_max {
            match self.max_latency_us.compare_exchange_weak(
                current_max,
                latency_us,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_max = actual,
            }
        }

        // Non-blocking sample acquisition
        if let Some(mut samples) = self.samples.try_write() {
            if samples.len() < MAX_SAMPLES_PER_WINDOW {
                samples.push(latency_us);
            }
        }
    }

    pub fn recompute_stats(&self) {
        let samples = {
            let mut s = self.samples.write();
            let copy = s.clone();
            if s.len() > 100 {
                s.drain(0..50);
            }
            copy
        };

        if samples.is_empty() {
            return;
        }

        let summary = LatencySummary::from_samples(samples);
        self.avg_latency_us.store(summary.avg_us, Ordering::Relaxed);
        self.p99_latency_us.store(summary.p99_us, Ordering::Relaxed);
    }
}

pub struct MetricsStore {
    start_time: Instant,
    pub total_requests: AtomicU64,
    pub success_requests: AtomicU64,
    pub client_errors: AtomicU64,
    pub server_errors: AtomicU64,
    pub current_concurrency: AtomicU64,
    pub current_rps: AtomicU64,
    last_sampled_requests: AtomicU64,

    // RingBuffers for 60-second real-time charts (x: seconds, y: value)
    pub rps_history: RwLock<VecDeque<(f64, f64)>>,
    pub latency_history: RwLock<VecDeque<(f64, f64)>>,

    // Recent latency samples in the current sampling window
    window_latencies: RwLock<Vec<u64>>,
    pub current_latency_summary: RwLock<LatencySummary>,

    // Endpoints map: "METHOD /path" -> EndpointStat
    pub endpoints: RwLock<HashMap<String, Arc<EndpointStat>>>,
}

impl MetricsStore {
    pub fn new() -> Arc<Self> {
        let mut initial_rps = VecDeque::with_capacity(MAX_HISTORY_POINTS);
        let mut initial_lat = VecDeque::with_capacity(MAX_HISTORY_POINTS);
        for i in 0..MAX_HISTORY_POINTS {
            initial_rps.push_back((i as f64, 0.0));
            initial_lat.push_back((i as f64, 0.0));
        }

        Arc::new(Self {
            start_time: Instant::now(),
            total_requests: AtomicU64::new(0),
            success_requests: AtomicU64::new(0),
            client_errors: AtomicU64::new(0),
            server_errors: AtomicU64::new(0),
            current_concurrency: AtomicU64::new(0),
            current_rps: AtomicU64::new(0),
            last_sampled_requests: AtomicU64::new(0),
            rps_history: RwLock::new(initial_rps),
            latency_history: RwLock::new(initial_lat),
            window_latencies: RwLock::new(Vec::with_capacity(1000)),
            current_latency_summary: RwLock::new(LatencySummary::default()),
            endpoints: RwLock::new(HashMap::new()),
        })
    }

    pub fn record_request(&self, method: &str, path: &str, status: u16, latency_us: u64) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        let is_error = status >= 400;

        if status < 400 {
            self.success_requests.fetch_add(1, Ordering::Relaxed);
        } else if status < 500 {
            self.client_errors.fetch_add(1, Ordering::Relaxed);
        } else {
            self.server_errors.fetch_add(1, Ordering::Relaxed);
        }

        // Non-blocking try_write for window latencies (avoids blocking HTTP threads)
        if let Some(mut lats) = self.window_latencies.try_write() {
            if lats.len() < MAX_SAMPLES_PER_WINDOW {
                lats.push(latency_us);
            }
        }

        // Fast endpoint lookup with read lock
        let key = format!("{} {}", method, path);
        if let Some(stat) = self.endpoints.read().get(&key) {
            stat.record(latency_us, is_error);
            return;
        }

        // Only acquire write lock on first discovery
        let mut map = self.endpoints.write();
        let stat = map
            .entry(key)
            .or_insert_with(|| Arc::new(EndpointStat::new(path.to_string(), method.to_string())));
        stat.record(latency_us, is_error);
    }

    /// Called every 1.0 second by the background Tokio sampler
    pub fn sample_interval(&self) {
        let current_total = self.total_requests.load(Ordering::Relaxed);
        let previous = self
            .last_sampled_requests
            .swap(current_total, Ordering::Relaxed);
        let rps = current_total.saturating_sub(previous);
        self.current_rps.store(rps, Ordering::Relaxed);

        // Compute summary for this second's window
        let samples = {
            let mut lats = self.window_latencies.write();
            let drained = lats.clone();
            lats.clear();
            drained
        };

        let summary = LatencySummary::from_samples(samples);
        *self.current_latency_summary.write() = summary;

        // Shift RingBuffer for RPS
        {
            let mut rps_hist = self.rps_history.write();
            if rps_hist.len() >= MAX_HISTORY_POINTS {
                rps_hist.pop_front();
            }
            for (idx, point) in rps_hist.iter_mut().enumerate() {
                point.0 = idx as f64;
            }
            let next_idx = rps_hist.len() as f64;
            rps_hist.push_back((next_idx, rps as f64));
        }

        // Shift RingBuffer for Latency (p95 in ms)
        {
            let mut lat_hist = self.latency_history.write();
            if lat_hist.len() >= MAX_HISTORY_POINTS {
                lat_hist.pop_front();
            }
            for (idx, point) in lat_hist.iter_mut().enumerate() {
                point.0 = idx as f64;
            }
            let p95_ms = summary.p95_us as f64 / 1000.0;
            let next_idx = lat_hist.len() as f64;
            lat_hist.push_back((next_idx, p95_ms));
        }

        // Recompute endpoint stats
        {
            let map = self.endpoints.read();
            for stat in map.values() {
                stat.recompute_stats();
            }
        }
    }

    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    pub fn reset(&self) {
        self.total_requests.store(0, Ordering::Relaxed);
        self.success_requests.store(0, Ordering::Relaxed);
        self.client_errors.store(0, Ordering::Relaxed);
        self.server_errors.store(0, Ordering::Relaxed);
        self.current_rps.store(0, Ordering::Relaxed);
        self.last_sampled_requests.store(0, Ordering::Relaxed);

        let mut rps = self.rps_history.write();
        rps.clear();
        for i in 0..MAX_HISTORY_POINTS {
            rps.push_back((i as f64, 0.0));
        }

        let mut lat = self.latency_history.write();
        lat.clear();
        for i in 0..MAX_HISTORY_POINTS {
            lat.push_back((i as f64, 0.0));
        }

        self.window_latencies.write().clear();
        *self.current_latency_summary.write() = LatencySummary::default();
        self.endpoints.write().clear();
    }
}
