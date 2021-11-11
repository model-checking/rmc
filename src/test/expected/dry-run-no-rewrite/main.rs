// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

// rmc-flags: --dry-run --unwind 4 --object-bits 8
// cbmc-flags: --unwind 2 --object-bits 10

// `--dry-run` causes RMC to print out commands instead of running them
// In `expected` you will find substrings of these commands because the
// concrete paths depend on your working directory.
pub fn main() {}
