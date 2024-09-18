// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z uninit-checks

#[repr(C)]
#[derive(kani::Arbitrary)]
struct S(u32, u8); // 5 bytes of data + 3 bytes of padding.

/// This checks that reading copied uninitialized bytes after a multi-byte copy fails an assertion.
#[kani::proof]
unsafe fn expose_padding_via_non_byte_copy() {
    let from: S = kani::any();
    let mut to: u64 = kani::any();

    let from_ptr = &from as *const S;
    let to_ptr = &mut to as *mut u64;

    // This should not cause UB since `copy` is untyped.
    std::ptr::copy(from_ptr as *const u64, to_ptr as *mut u64, 1);

    // This reads uninitialized bytes, which is UB.
    let padding: u64 = std::ptr::read(to_ptr);
}
