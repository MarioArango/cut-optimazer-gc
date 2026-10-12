#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
echo "==> Rust format"
cargo fmt --all
cargo fmt --all -- --check
echo "==> Rust tests"
cargo test --workspace
echo "==> CLI model examples"
cargo run --quiet -p optimizer-cli -- run --input examples/validate-model.json
cargo run --quiet -p optimizer-cli -- run --input examples/validate-strip-stack.json
echo "==> Stage 4 geometry and preprocessing"
cargo run --quiet -p optimizer-cli -- run --input examples/preprocess-model.json
cargo run --quiet -p optimizer-cli -- run --input examples/preprocess-ineligible-piece.json
echo "==> Stage 1 regression"
cargo run --quiet -p optimizer-cli -- run --input examples/system-check.json
echo "==> Stage 4 geometry"
cargo run --quiet -p optimizer-cli -- run --input examples/geometry-vertical-split.json
cargo run --quiet -p optimizer-cli -- run --input examples/geometry-horizontal-split.json
cargo run --quiet -p optimizer-cli -- run --input examples/geometry-trim.json
echo "==> Stage 5 guillotine pattern evaluation"
cargo run --quiet -p optimizer-cli -- run --input examples/guillotine-vh.json
cargo run --quiet -p optimizer-cli -- run --input examples/guillotine-hv.json
cargo run --quiet -p optimizer-cli -- run --input examples/guillotine-vhv.json
cargo run --quiet -p optimizer-cli -- run --input examples/guillotine-hvh.json
echo "==> Stage 6 independent complete-layout validation"
cargo run --quiet -p optimizer-cli -- run --input examples/layout-valid.json
cargo run --quiet -p optimizer-cli -- run --input examples/layout-invalid-missing.json
cargo run --quiet -p optimizer-cli -- run --input examples/layout-invalid-duplicate-stock.json
echo "==> OK"
