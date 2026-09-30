#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

docker compose exec -T postgres psql \
  -U ferris \
  -d ferris_bench \
  < benchmarks/postgres/seed.sql

echo "PostgreSQL benchmark dataset ready: 100000 users."
