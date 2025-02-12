// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z uninit-checks

use std::ptr;
use std::ptr::addr_of;

/// The layout of this enum is variable, so Kani cannot check memory initialization statically.
#[repr(C)]
enum E1 {
    A(u16, u8),
    B(u16),
}

/// The layout of this enum is variable, but both of the arms have the same padding, so Kani should
/// support that.
#[repr(C)]
enum E2 {
    A(u16),
    B(u8, u8),
}

#[kani::proof]
fn access_padding_unsupported() {
    let s = E1::A(0, 0);
    let ptr: *const u8 = addr_of!(s) as *const u8;
}

#[kani::proof]
fn access_padding_supported() {
    let s = E2::A(0);
    let ptr: *const u8 = addr_of!(s) as *const u8;
}
