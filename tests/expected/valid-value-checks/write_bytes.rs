// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z valid-value-checks
//! Check that Kani can identify UB when using write_bytes for initializing a variable.
#![feature(core_intrinsics)]

#[kani::proof]
pub fn check_invalid_write() {
    let mut val = 'a';
    let ptr = &mut val as *mut char;
    // Should fail given that we wrote invalid value to array of char.
    unsafe { std::intrinsics::write_bytes(ptr, kani::any(), 1) };
}

#[kani::proof]
pub fn check_valid_write() {
    let mut val = 10u128;
    let ptr = &mut val as *mut _;
    unsafe { std::intrinsics::write_bytes(ptr, kani::any(), 1) };
}
