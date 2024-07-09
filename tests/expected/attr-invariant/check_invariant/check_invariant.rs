// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Check that the `kani::invariant` attribute is applied and the associated
//! invariant can be checked through an `is_safe` call.

extern crate kani;
use kani::Invariant;

#[derive(kani::Arbitrary)]
#[kani::invariant(x.is_safe() && y.is_safe())]
struct Point {
    x: i32,
    y: i32,
}

#[kani::proof]
fn check_invariant() {
    let point: Point = kani::any();
    assert!(point.is_safe());
}
