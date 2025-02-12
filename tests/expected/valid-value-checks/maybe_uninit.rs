// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z valid-value-checks
//! Check that Kani can identify UB when converting from a maybe uninit().

use std::mem::MaybeUninit;
use std::num::NonZeroI64;

#[kani::proof]
pub fn check_valid_zeroed() {
    let maybe = MaybeUninit::zeroed();
    let val: u128 = unsafe { maybe.assume_init() };
    assert_eq!(val, 0);
}

#[kani::proof]
pub fn check_invalid_zeroed() {
    let maybe = MaybeUninit::zeroed();
    let _val: NonZeroI64 = unsafe { maybe.assume_init() };
}
