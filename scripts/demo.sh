#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run --quiet -p optimizer-cli -- capabilities
cargo run --quiet -p optimizer-cli -- run --input examples/system-check.json
cargo run --quiet -p optimizer-cli -- validate --input examples/validate-model.json
cargo run --quiet -p optimizer-cli -- run --input examples/validate-model.json
cargo run --quiet -p optimizer-cli -- run --input examples/preprocess-model.json
cargo run --quiet -p optimizer-cli -- run --input examples/preprocess-grain-conflict.json
cargo run --quiet -p optimizer-cli -- run --input examples/preprocess-insufficient-stock.json
cargo run --quiet -p optimizer-cli -- run --input examples/geometry-vertical-split.json
cargo run --quiet -p optimizer-cli -- run --input examples/geometry-horizontal-split.json
cargo run --quiet -p optimizer-cli -- run --input examples/geometry-trim.json
cargo run --quiet -p optimizer-cli -- run --input examples/guillotine-vh.json
cargo run --quiet -p optimizer-cli -- run --input examples/guillotine-vhv.json
