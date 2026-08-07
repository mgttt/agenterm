#!/usr/bin/env sh
# Dedicated rh AOT / backend qualification lane (Linux/macOS).
set -eu

ROOT="$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "== build agenterm-rh binary =="
cargo build --locked --bin agenterm-rh

echo "== agenterm-rh crate =="
cargo test -p agenterm-rh --locked

echo "== rh integration tests =="
cargo test --locked \
  --test rh_aot_smoke \
  --test rh_aot_ci_policy \
  --test rh_regression \
  --test rh_backend \
  --test rh_corpus \
  --test rh_framed_worker \
  --test rh_cli_forward \
  --test rh_standalone_cli \
  --test rh_native_task \
  --test script_check_many \
  --test performance_experiment_policy

echo "== rh host + cache lib tests =="
cargo test -p agenterm --locked --lib script_rh_host
cargo test -p agenterm --locked --lib script_rh_cache

echo "PASS rh-check"
