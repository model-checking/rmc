// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This tests that the autoharness feature doesn't generate harnesses for functions outside the local crate.

use other_crate;

fn yes_harness(x: u8) -> u8 {
    x
}
