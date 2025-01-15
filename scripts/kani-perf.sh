#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

set -o pipefail
set -o nounset

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
KANI_DIR=$SCRIPT_DIR/..

# Build Kani using release mode.
cargo build-dev -- --release

PERF_DIR="${KANI_DIR}/tests/perf"

# Copy expected files from overlay directories
to_delete=
for overlay_dir in ${PERF_DIR}/overlays/*/; do
  orig_dir=$(basename ${overlay_dir})
  echo "Copying overlays for $orig_dir"
  copy_output=$(cp -r -v ${overlay_dir}* ${PERF_DIR}/${orig_dir}/)
  copied_files=$(echo ${copy_output} | rev | cut -d' ' -f 1 | rev | tr -d "'")
  # Add to the list of files to delete
  to_delete="${to_delete} ${copied_files}"
done

suite="perf"
mode="cargo-kani-test"
echo "Check compiletest suite=$suite mode=$mode"
cargo run -p compiletest -- --suite $suite --mode $mode --no-fail-fast \
  --kani-flag="--enable-unstable" --kani-flag="--cbmc-args" --kani-flag="--verbosity" --kani-flag="9"
exit_code=$?

echo "Cleaning up..."
rm ${to_delete}

echo
if [ $exit_code -eq 0 ]; then
  echo "All Kani perf tests completed successfully."
else
  echo "***Kani perf tests failed."
fi
echo
exit $exit_code
