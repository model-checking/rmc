// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z mem-predicates
//! Check that Kani's memory predicates work for fat pointers.

extern crate kani;

use kani::mem::{can_dereference, can_write};

mod valid_access {
    use super::*;
    #[kani::proof]
    pub fn check_valid_dyn_ptr() {
        let mut var = 10u8;
        let fat_ptr: *mut dyn PartialEq<u8> = &mut var as *mut _;
        assert!(can_write(fat_ptr));
    }

    #[kani::proof]
    pub fn check_valid_slice_ptr() {
        let array = ['a', 'b', 'c'];
        let slice = &array as *const [char];
        assert!(can_dereference(slice));
        assert_eq!(unsafe { &*slice }[0], 'a');
        assert_eq!(unsafe { &*slice }[2], 'c');
    }

    #[kani::proof]
    pub fn check_valid_zst() {
        let slice_ptr = Vec::<char>::new().as_slice() as *const [char];
        assert!(can_dereference(slice_ptr));

        let str_ptr = String::new().as_str() as *const str;
        assert!(can_dereference(str_ptr));
    }
}

mod invalid_access {
    use super::*;
    #[kani::proof]
    pub fn check_invalid_dyn_ptr() {
        let raw_ptr: *const dyn PartialEq<u8> = unsafe { new_dead_ptr::<u8>(0) };
        assert!(can_dereference(raw_ptr));
    }

    #[kani::proof]
    pub fn check_invalid_slice_ptr() {
        let raw_ptr: *const [char] = unsafe { new_dead_ptr::<[char; 2]>(['a', 'b']) };
        assert!(can_dereference(raw_ptr));
    }

    #[kani::proof]
    #[kani::should_panic]
    pub fn check_invalid_slice_len() {
        let array = [10usize; 10];
        let invalid: *const [usize; 11] = &array as *const [usize; 10] as *const [usize; 11];
        let ptr: *const [usize] = invalid as *const _;
        assert!(can_dereference(ptr));
    }

    unsafe fn new_dead_ptr<T>(val: T) -> *const T {
        let local = val;
        &local as *const _
    }
}
