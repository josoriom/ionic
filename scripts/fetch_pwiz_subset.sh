#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PWIZ_DIR="$ROOT_DIR/pwiz"
REMOTE_URL="https://github.com/ProteoWizard/pwiz.git"

if [[ ! -d "$PWIZ_DIR/.git" ]]; then
  git init "$PWIZ_DIR" >/dev/null
  git -C "$PWIZ_DIR" remote add origin "$REMOTE_URL"
fi

if ! git -C "$PWIZ_DIR" remote get-url origin >/dev/null 2>&1; then
  git -C "$PWIZ_DIR" remote add origin "$REMOTE_URL"
fi

git -C "$PWIZ_DIR" config core.sparseCheckout true
git -C "$PWIZ_DIR" sparse-checkout init --cone
git -C "$PWIZ_DIR" sparse-checkout set pwiz/data/msdata example_data

git -C "$PWIZ_DIR" fetch --depth=1 --filter=blob:none origin HEAD
git -C "$PWIZ_DIR" checkout --detach FETCH_HEAD

echo "pwiz subset ready at: $PWIZ_DIR"
