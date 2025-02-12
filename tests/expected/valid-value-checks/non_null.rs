// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z valid-value-checks
//! Check that Kani can identify UB when unsafely writing to NonNull.

use std::num::NonZeroU8;
use std::ptr::{self, NonNull};

#[kani::proof]
pub fn check_invalid_value() {
    let _ = unsafe { NonNull::new_unchecked(ptr::null_mut::<u8>()) };
}

#[kani::proof]
pub fn check_invalid_value_cfg() {
    let nn = unsafe { NonNull::new_unchecked(ptr::null_mut::<u8>()) };
    // This should be unreachable. TODO: Make this expected test.
    assert_ne!(unsafe { nn.as_ref() }, &10);
}

#[kani::proof]
pub fn check_valid_dangling() {
    let _ = unsafe { NonNull::new_unchecked(4 as *mut u32) };
}
