// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-verify-fail

// ANCHOR: code
fn find_midpoint(low: u32, high: u32) -> u32 {
    return (low + high) / 2;
}
// ANCHOR_END: code

// ANCHOR: kani
#[cfg(kani)]
#[kani::proof]
fn midpoint_overflow() {
    let a: u32 = kani::any();
    let b: u32 = kani::any();
    find_midpoint(a, b);
}
// ANCHOR_END: kani
