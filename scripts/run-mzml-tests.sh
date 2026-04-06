#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ ! -f "pwiz/example_data/tiny.pwiz.1.1.mzML" ]]; then
  ./scripts/fetch_pwiz_subset.sh
fi

cargo nextest run --workspace --all-features
