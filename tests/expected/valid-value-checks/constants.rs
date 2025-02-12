// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z valid-value-checks
//! Check that Kani can identify UB when it is reading from a constant.
//! Note that this UB will be removed for `-Z mir-opt-level=2`

#[kani::proof]
fn transmute_valid_bool() {
    let _b = unsafe { std::mem::transmute::<u8, bool>(1) };
}

#[kani::proof]
fn cast_to_valid_char() {
    let _c = unsafe { *(&100u32 as *const u32 as *const char) };
}

#[kani::proof]
fn cast_to_valid_offset() {
    let val = [100u32, 80u32];
    let _c = unsafe { *(&val as *const [u32; 2] as *const [char; 2]) };
}

#[kani::proof]
fn transmute_invalid_bool() {
    let _b = unsafe { std::mem::transmute::<u8, bool>(2) };
}

#[kani::proof]
fn cast_to_invalid_char() {
    let _c = unsafe { *(&u32::MAX as *const u32 as *const char) };
}

#[kani::proof]
fn cast_to_invalid_offset() {
    let val = [100u32, u32::MAX];
    let _c = unsafe { *(&val as *const [u32; 2] as *const [char; 2]) };
}
