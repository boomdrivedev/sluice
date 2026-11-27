#!/usr/bin/env bash
# The check-e2e tier: e2e.sh's S1 to S4 with the sluice and wt-submit built
# from this tree. The scratch repository is removed when the run passes and
# kept, with its path printed, when it fails.
set -euo pipefail

cd "$(dirname "$0")/.."
# Its own target folder, which its task names, so no other task's build
# writes the sluice it runs (crates/sluice/moon.yml).
target="${CARGO_TARGET_DIR:?the e2e task names its target folder}"
cargo build -p sluice --bins
PATH="$PWD/$target/debug:$PATH"
export PATH
scratch=$(mktemp -d "${TMPDIR:-/tmp}/e2e.XXXXXX")
if ! bash e2e/e2e.sh "$scratch/repo" sluice; then
  echo "check-e2e: failed; its repository is in $scratch/repo" >&2
  exit 1
fi
rm -rf "$scratch"
