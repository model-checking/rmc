// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#[kani::proof]
#[kani::should_panic]
pub fn rust_ub_fails() {
    let ptr = 0 as *const u32;
    let _invalid_ref = unsafe { &*ptr };
}

#[kani::proof]
#[kani::should_panic]
pub fn rust_ub_should_fail() {
    let ptr = 10 as *const u32;
    let _invalid_read = unsafe { *ptr };
}
