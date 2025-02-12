// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z uninit-checks

use std::ptr;
use std::ptr::addr_of;

/// The layout of this enum is the following (D = data, P = padding):
///  0  1  2  3  4  5  6  7
/// [D, D, D, D, D, D, D, P]
///  ----------  -------
///   \_ tag (i32)  \_ A(u16, u8)
#[repr(C)]
enum E {
    A(u16, u8),
}

#[kani::proof]
fn access_padding_init() {
    let s = E::A(0, 0);
    let ptr: *const u8 = addr_of!(s) as *const u8;
    let at_0 = unsafe { *(ptr.add(0)) };
    let at_4 = unsafe { *(ptr.add(4)) };
}

#[kani::proof]
fn access_padding_uninit() {
    let s = E::A(0, 0);
    let ptr: *const u8 = addr_of!(s) as *const u8;
    let at_7 = unsafe { *(ptr.add(7)) };
}
