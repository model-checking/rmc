// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-verify-fail

// Checks that the pointer computed with `offset` causes a failure if it's
// dereferenced outside the object bounds
#![feature(core_intrinsics)]
use std::intrinsics::offset;

#[kani::proof]
fn test_offset() {
    let s: &str = "123";
    let ptr: *const u8 = s.as_ptr();

    unsafe {
        let x = *offset(ptr, 4) as char;
    }
}
