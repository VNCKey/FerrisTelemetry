use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;
use std::time::Instant;

use crate::metrics::MetricsStore;

pub async fn telemetry_middleware(
    State(metrics): State<Arc<MetricsStore>>,
    req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    let response = next.run(req).await;

    let latency_us = start.elapsed().as_micros() as u64;
    let status = response.status().as_u16();

    metrics.record_request(&method, &path, status, latency_us);

    response
}
