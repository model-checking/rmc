// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z uninit-checks

use std::ptr;
use std::ptr::addr_of;

/// The layout of this enum is the following (D = data, P = padding):
///  0  1  2  3  4  5  6  7  8  9  A  B  C  D  E  F
/// [D, D, D, D, P, P, P, P, D, D, D, D, D, D, D, D]
///  ----------              ----------------------
///      \_ tag (i32)                   \_ A(u64)
#[repr(C)]
enum E {
    A(u64),
}

#[kani::proof]
fn access_padding_init() {
    let s = E::A(0);
    let ptr: *const u8 = addr_of!(s) as *const u8;
    let at_0 = unsafe { *(ptr.add(0)) };
    let at_3 = unsafe { *(ptr.add(3)) };
    let at_9 = unsafe { *(ptr.add(9)) };
}

#[kani::proof]
fn access_padding_uninit() {
    let s = E::A(0);
    let ptr: *const u8 = addr_of!(s) as *const u8;
    let at_4 = unsafe { *(ptr.add(4)) };
    let at_7 = unsafe { *(ptr.add(7)) };
}
