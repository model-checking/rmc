// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// Required so we can use kani_macros attributes.
#![feature(register_tool)]
#![register_tool(kanitool)]
// Used for rustc_diagnostic_item.
// Note: We could use a kanitool attribute instead.
#![feature(rustc_attrs)]
// This is required for the optimized version of `any_array()`
#![feature(generic_const_exprs)]
#![allow(incomplete_features)]
// Used to model simd.
#![feature(repr_simd)]
// Features used for tests only.
#![cfg_attr(test, feature(core_intrinsics, portable_simd))]
// Required for `rustc_diagnostic_item` and `core_intrinsics`
#![allow(internal_features)]
// Required for implementing memory predicates.
#![feature(ptr_metadata)]

pub mod arbitrary;
#[cfg(feature = "concrete_playback")]
mod concrete_playback;
pub mod futures;
pub mod mem;
pub mod slice;
pub mod tuple;
pub mod vec;

#[doc(hidden)]
pub mod internal;

mod models;

#[cfg(feature = "concrete_playback")]
pub use concrete_playback::concrete_playback_run;

#[cfg(not(feature = "concrete_playback"))]
/// NOP `concrete_playback` for type checking during verification mode.
pub fn concrete_playback_run<F: Fn()>(_: Vec<Vec<u8>>, _: F) {
    unreachable!("Concrete playback does not work during verification")
}
pub use futures::{block_on, block_on_with_spawn, spawn, yield_now, RoundRobin};

// Declare common Kani features.
kani_core::kani_lib!(kani);

/// `implies!(premise => conclusion)` means that if the `premise` is true, so
/// must be the `conclusion`.
///
/// This simply expands to `!premise || conclusion` and is intended to make checks more readable,
/// as the concept of an implication is more natural to think about than its expansion.
#[macro_export]
macro_rules! implies {
    ($premise:expr => $conclusion:expr) => {
        !($premise) || ($conclusion)
    };
}

/// A macro to check if a condition is satisfiable at a specific location in the
/// code.
///
/// # Example 1:
///
/// ```rust
/// let mut set: BTreeSet<i32> = BTreeSet::new();
/// set.insert(kani::any());
/// set.insert(kani::any());
/// // check if the set can end up with a single element (if both elements
/// // inserted were the same)
/// kani::cover!(set.len() == 1);
/// ```
/// The macro can also be called without any arguments to check if a location is
/// reachable.
///
/// # Example 2:
///
/// ```rust
/// match e {
///     MyEnum::A => { /* .. */ }
///     MyEnum::B => {
///         // make sure the `MyEnum::B` variant is possible
///         kani::cover!();
///         // ..
///     }
/// }
/// ```
///
/// A custom message can also be passed to the macro.
///
/// # Example 3:
///
/// ```rust
/// kani::cover!(x > y, "x can be greater than y")
/// ```
#[macro_export]
macro_rules! cover {
    () => {
        kani::cover(true, "cover location");
    };
    ($cond:expr $(,)?) => {
        kani::cover($cond, concat!("cover condition: ", stringify!($cond)));
    };
    ($cond:expr, $msg:literal) => {
        kani::cover($cond, $msg);
    };
}

// Used to bind `core::assert` to a different name to avoid possible name conflicts if a
// crate uses `extern crate std as core`. See
// https://github.com/model-checking/kani/issues/1949 and https://github.com/model-checking/kani/issues/2187
#[doc(hidden)]
#[cfg(not(feature = "concrete_playback"))]
pub use core::assert as __kani__workaround_core_assert;

pub mod contracts;
