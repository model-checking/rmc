// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Check the compilation error for the invariant attribute helper when an
//! argument isn't provided.

extern crate kani;
use kani::Invariant;

#[derive(kani::Arbitrary)]
#[derive(kani::Invariant)]
struct PositivePoint {
    #[invariant]
    x: i32,
    #[invariant(self.y >= 0)]
    y: i32,
}
