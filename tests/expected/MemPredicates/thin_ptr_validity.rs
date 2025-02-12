// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z mem-predicates
//! Check that Kani's memory predicates work for thin pointers.

extern crate kani;

use kani::mem::can_dereference;
use std::ptr::NonNull;

mod valid_access {
    use super::*;
    #[kani::proof]
    pub fn check_dangling_zst() {
        let dangling = NonNull::<()>::dangling().as_ptr();
        assert!(can_dereference(dangling));

        let vec_ptr = Vec::<()>::new().as_ptr();
        assert!(can_dereference(vec_ptr));

        let dangling = NonNull::<[char; 0]>::dangling().as_ptr();
        assert!(can_dereference(dangling));
    }

    #[kani::proof]
    pub fn check_valid_array() {
        let array = ['a', 'b', 'c'];
        assert!(can_dereference(&array));
    }
}

mod invalid_access {
    use super::*;
    #[kani::proof]
    pub fn check_invalid_ptr() {
        let raw_ptr = unsafe { new_dead_ptr::<u8>(0) };
        assert!(!can_dereference(raw_ptr));
    }

    #[kani::proof]
    pub fn check_invalid_array() {
        let raw_ptr = unsafe { new_dead_ptr::<[char; 2]>(['a', 'b']) };
        assert!(can_dereference(raw_ptr));
    }

    #[kani::proof]
    pub fn check_invalid_zst() {
        let raw_ptr: *const [char; 0] =
            unsafe { new_dead_ptr::<[char; 2]>(['a', 'b']) } as *const _;
        // ZST pointer are always valid
        assert!(can_dereference(raw_ptr));
    }

    unsafe fn new_dead_ptr<T>(val: T) -> *const T {
        let local = val;
        &local as *const _
    }
}
