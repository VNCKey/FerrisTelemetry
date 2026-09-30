use parking_lot::RwLock;
use reqwest::Client;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::metrics::MetricsStore;

pub struct LoadGenerator {
    pub is_active: AtomicBool,
    pub concurrency: AtomicUsize,
    base_url: String,
    client: Client,
    pub status_text: RwLock<String>,
    metrics: Arc<MetricsStore>,
}

impl LoadGenerator {
    pub fn new(port: u16, metrics: Arc<MetricsStore>) -> Arc<Self> {
        let client = Client::builder()
            .timeout(Duration::from_millis(1500))
            .pool_max_idle_per_host(50)
            .tcp_nodelay(true)
            .build()
            .unwrap_or_default();

        Arc::new(Self {
            is_active: AtomicBool::new(false),
            concurrency: AtomicUsize::new(10),
            base_url: format!("http://127.0.0.1:{}", port),
            client,
            status_text: RwLock::new("Pausado (presiona [s] para iniciar)".to_string()),
            metrics,
        })
    }

    pub fn toggle(&self) -> bool {
        let current = self.is_active.load(Ordering::SeqCst);
        let next = !current;
        self.is_active.store(next, Ordering::SeqCst);

        let mut status = self.status_text.write();
        if next {
            *status = format!(
                "Generando carga activa ({} workers)",
                self.concurrency.load(Ordering::Relaxed)
            );
        } else {
            *status = "Pausado (presiona [s] para iniciar)".to_string();
        }
        next
    }

    pub fn increase_concurrency(&self) -> usize {
        let current = self.concurrency.load(Ordering::Relaxed);
        let next = (current + 5).min(100);
        self.concurrency.store(next, Ordering::Relaxed);
        if self.is_active.load(Ordering::Relaxed) {
            *self.status_text.write() = format!("Generando carga activa ({} workers)", next);
        }
        next
    }

    pub fn decrease_concurrency(&self) -> usize {
        let current = self.concurrency.load(Ordering::Relaxed);
        let next = if current > 5 { current - 5 } else { 1 };
        self.concurrency.store(next, Ordering::Relaxed);
        if self.is_active.load(Ordering::Relaxed) {
            *self.status_text.write() = format!("Generando carga activa ({} workers)", next);
        }
        next
    }

    /// Background loop running worker tasks
    pub async fn start_loop(self: Arc<Self>) {
        let endpoints = [
            "/api/v1/users",
            "/api/v1/compute",
            "/api/v1/plain",
            "/api/v1/health",
        ];

        let mut counter = 0usize;

        loop {
            if !self.is_active.load(Ordering::Relaxed) {
                sleep(Duration::from_millis(150)).await;
                continue;
            }

            let concurrency = self.concurrency.load(Ordering::Relaxed);
            let mut batch_handles = Vec::with_capacity(concurrency);

            for _ in 0..concurrency {
                let ep = endpoints[counter % endpoints.len()];
                counter += 1;
                let url = format!("{}{}", self.base_url, ep);
                let client = self.client.clone();
                let metrics = self.metrics.clone();

                batch_handles.push(tokio::spawn(async move {
                    let req_start = Instant::now();
                    let res = client.get(&url).send().await;
                    let latency_us = req_start.elapsed().as_micros() as u64;
                    let status = match res {
                        Ok(r) => r.status().as_u16(),
                        Err(_) => 500,
                    };
                    metrics.record_request("GET", ep, status, latency_us);
                }));
            }

            for h in batch_handles {
                let _ = h.await;
            }

            // Small throttle between burst cycles
            sleep(Duration::from_millis(30)).await;
        }
    }
}
