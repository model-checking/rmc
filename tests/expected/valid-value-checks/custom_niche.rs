// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Z valid-value-checks
//! Check that Kani can identify UB when using niche attribute for a custom operation.
#![feature(rustc_attrs)]

use std::mem::size_of;
use std::{mem, ptr};

/// A possible implementation for a system of rating that defines niche.
/// A Rating represents the number of stars of a given product (1..=5).
#[rustc_layout_scalar_valid_range_start(1)]
#[rustc_layout_scalar_valid_range_end(5)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rating {
    stars: u8,
}

impl kani::Arbitrary for Rating {
    fn any() -> Self {
        let stars = kani::any_where(|s: &u8| *s >= 1 && *s <= 5);
        unsafe { Rating { stars } }
    }
}

impl Rating {
    /// Buggy version of new. Note that this still creates an invalid Rating.
    ///
    /// This is because `then_some` eagerly create the Rating value before assessing the condition.
    /// Even though the value is never used, it is still considered UB.
    pub fn new(value: u8) -> Option<Rating> {
        (value > 0 && value <= 5).then_some(unsafe { Rating { stars: value } })
    }

    pub unsafe fn new_unchecked(stars: u8) -> Rating {
        Rating { stars }
    }
}

#[kani::proof]
pub fn check_new_with_ub() {
    assert_eq!(Rating::new(10), None);
}

#[kani::proof]
pub fn check_unchecked_new_ub() {
    let val = kani::any();
    assert_eq!(unsafe { Rating::new_unchecked(val).stars }, val);
}

#[kani::proof]
pub fn check_new_with_ub_limits() {
    let stars = kani::any_where(|s: &u8| *s == 0 || *s > 5);
    let _ = Rating::new(stars);
}

#[kani::proof]
pub fn check_invalid_dereference() {
    let any: u8 = kani::any();
    let _rating: Rating = unsafe { *(&any as *const _ as *const _) };
}

#[kani::proof]
pub fn check_invalid_transmute() {
    let any: u8 = kani::any();
    let _rating: Rating = unsafe { mem::transmute(any) };
}

#[kani::proof]
pub fn check_invalid_transmute_copy() {
    let any: u8 = kani::any();
    let _rating: Rating = unsafe { mem::transmute_copy(&any) };
}

/// This code does not trigger UB, and verification should succeed.
///
/// FIX-ME: This is not supported today, and we fail due to unsupported check.
#[kani::proof]
pub fn check_copy_nonoverlap() {
    let stars = kani::any_where(|s: &u8| *s == 0 || *s > 5);
    let mut rating: Rating = kani::any();
    unsafe { ptr::copy_nonoverlapping(&stars as *const _ as *const Rating, &mut rating, 1) };
}

#[kani::proof]
pub fn check_copy_nonoverlap_ub() {
    let any: u8 = kani::any();
    let mut rating: Rating = kani::any();
    unsafe { ptr::copy_nonoverlapping(&any as *const _ as *const Rating, &mut rating, 1) };
}

#[kani::proof]
pub fn check_invalid_increment() {
    let mut orig: Rating = kani::any();
    unsafe { orig.stars += 1 };
}

#[kani::proof]
pub fn check_valid_increment() {
    let mut orig: Rating = kani::any();
    kani::assume(orig.stars < 5);
    unsafe { orig.stars += 1 };
}

/// Check that the compiler relies on valid value range of Rating to implement niche optimization.
#[kani::proof]
pub fn check_niche() {
    assert_eq!(size_of::<Rating>(), size_of::<Option<Rating>>());
    assert_eq!(size_of::<Rating>(), size_of::<Option<Option<Rating>>>());
}
