#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

cargo build --release --bin ferris_telemetry --bin axum_server --bin actix_server
(
  cd benchmarks/go_fiber
  go build -trimpath -ldflags="-s -w" -o fiber_server .
)

# Cargo/Go pueden reutilizar un artefacto válido aunque el manifiesto o un
# archivo de configuración tenga una fecha más reciente. La Arena usa las
# fechas como protección contra binarios viejos, así que sincronizamos el
# timestamp después de una compilación exitosa.
touch target/release/ferris_telemetry \
  target/release/axum_server \
  target/release/actix_server \
  benchmarks/go_fiber/fiber_server

echo "Benchmark targets built in release mode."
