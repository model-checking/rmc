// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z valid-value-checks

//! Check that Kani can identify UB after writing an invalid value.
//! Writing invalid bytes is not UB as long as the incorrect value is not read.
//! However, we over-approximate for sake of simplicity and performance.

use std::num::NonZeroU8;

#[kani::proof]
pub fn write_invalid_bytes_no_ub_with_spurious_cex() {
    let mut non_zero: NonZeroU8 = kani::any();
    let dest = &mut non_zero as *mut _;
    unsafe { std::intrinsics::write_bytes(dest, 0, 1) };
}

#[kani::proof]
pub fn write_valid_before_read() {
    let mut non_zero: NonZeroU8 = kani::any();
    let mut non_zero_2: NonZeroU8 = kani::any();
    let dest = &mut non_zero as *mut _;
    unsafe { std::intrinsics::write_bytes(dest, 0, 1) };
    unsafe { std::intrinsics::write_bytes(dest, non_zero_2.get(), 1) };
    assert_eq!(non_zero, non_zero_2)
}

#[kani::proof]
pub fn read_invalid_is_ub() {
    let mut non_zero: NonZeroU8 = kani::any();
    let dest = &mut non_zero as *mut _;
    unsafe { std::intrinsics::write_bytes(dest, 0, 1) };
    assert_eq!(non_zero.get(), 0)
}
