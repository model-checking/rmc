#!/usr/bin/env bash
# Copyright Kani Contributors
# SPDX-License-Identifier: Apache-2.0 OR MIT

set -e

# Run this inside a tmp folder in the current directory
OUT_DIR=output_dir
# Ensure output folder is clean
rm -rf ${OUT_DIR}
mkdir output_dir
# Move the original source to the output folder since it will be modified
cp test.rs ${OUT_DIR}
pushd $OUT_DIR

cargo run -p scanner test.rs --crate-type lib
wc -l *csv

popd
#rm -rf ${OUT_DIR}
