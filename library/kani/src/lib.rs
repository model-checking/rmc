// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
#![feature(rustc_attrs)] // Used for rustc_diagnostic_item.
#![feature(min_specialization)] // Used for default implementation of Arbitrary.
#![allow(incomplete_features)]
// Used for getting the size of generic types.
// See this issue for more details: https://github.com/rust-lang/rust/issues/44580.
// Note: We can remove this feature after we remove the following deprecated functions:
// kani::any_raw, slice::AnySlice::new_raw(), slice::any_raw_slice(), (T: Invariant)::any().
#![feature(generic_const_exprs)]

pub mod arbitrary;
pub mod futures;
pub mod invariant;
pub mod slice;
pub mod vec;

pub use arbitrary::Arbitrary;
pub use futures::block_on;
#[allow(deprecated)]
pub use invariant::Invariant;

#[cfg(feature = "exe_trace")]
use std::sync::Mutex;
/// DET_VALS_LOCK is used by each playback test case to ensure that only a single thread is modifying DET_VALS at once.
/// We need to separate the lock from the data because there's no other way to pass the data from
/// kani::exe_trace_run() to kani::any_raw_internal() while still holding the lock.
#[cfg(feature = "exe_trace")]
static DET_VALS_LOCK: Mutex<()> = Mutex::new(());
#[cfg(feature = "exe_trace")]
static mut DET_VALS: Vec<Vec<u8>> = Vec::new();

/// Creates an assumption that will be valid after this statement run. Note that the assumption
/// will only be applied for paths that follow the assumption. If the assumption doesn't hold, the
/// program will exit successfully.
///
/// # Example:
///
/// The code snippet below should never panic.
///
/// ```rust
/// let i : i32 = kani::any();
/// kani::assume(i > 10);
/// if i < 0 {
///   panic!("This will never panic");
/// }
/// ```
///
/// The following code may panic though:
///
/// ```rust
/// let i : i32 = kani::any();
/// assert!(i < 0, "This may panic and verification should fail.");
/// kani::assume(i > 10);
/// ```
#[inline(never)]
#[rustc_diagnostic_item = "KaniAssume"]
pub fn assume(_cond: bool) {
    #[cfg(feature = "exe_trace")]
    assert!(_cond);
}

/// Creates an assertion of the specified condition and message.
///
/// # Example:
///
/// ```rust
/// let x: bool = kani::any();
/// let y = !x;
/// kani::assert(x || y, "ORing a boolean variable with its negation must be true")
/// ```
#[inline(never)]
#[rustc_diagnostic_item = "KaniAssert"]
pub fn assert(_cond: bool, _msg: &'static str) {
    #[cfg(feature = "exe_trace")]
    assert!(_cond, "{}", _msg);
}

/// This creates an symbolic *valid* value of type `T`. You can assign the return value of this
/// function to a variable that you want to make symbolic.
///
/// # Example:
///
/// In the snippet below, we are verifying the behavior of the function `fn_under_verification`
/// under all possible `NonZeroU8` input values, i.e., all possible `u8` values except zero.
///
/// ```rust
/// let inputA = kani::any::<std::num::NonZeroU8>();
/// fn_under_verification(inputA);
/// ```
///
/// Note: This is a safe construct and can only be used with types that implement the `Arbitrary`
/// trait. The Arbitrary trait is used to build a symbolic value that represents all possible
/// valid values for type `T`.
#[inline(always)]
pub fn any<T: Arbitrary>() -> T {
    T::any()
}

/// This function creates an unconstrained value of type `T`. This may result in an invalid value.
///
/// # Safety
///
/// This function is unsafe and it may represent invalid `T` values which can lead to many
/// undesirable undefined behaviors. Users must guarantee that an unconstrained symbolic value
/// for type T only represents valid values.
///
/// # Deprecation
///
/// We have decided to deprecate this function due to the fact that its result can be the source
/// of undefined behavior.
#[inline(never)]
#[deprecated(
    since = "0.8.0",
    note = "This function may return symbolic values that don't respects the language type invariants."
)]
#[doc(hidden)]
pub unsafe fn any_raw<T>() -> T
where
    // This generic_const_exprs feature lets Rust know the size of generic T.
    [(); std::mem::size_of::<T>()]:,
{
    any_raw_internal::<T, { std::mem::size_of::<T>() }>()
}

/// This function will replace `any_raw` that has been deprecated and it should only be used
/// internally when we can guarantee that it will not trigger any undefined behavior.
/// This function is also used to find deterministic bytes in the CBMC output trace.
#[inline(never)]
pub(crate) unsafe fn any_raw_internal<T, const SIZE_T: usize>() -> T {
    #[cfg(feature = "exe_trace")]
    {
        // This code will only run when our thread's exe_trace_run() fn holds the lock.
        let next_det_val = DET_VALS.pop().expect("Not enough det vals found");
        let next_det_val_len = next_det_val.len();
        let bytes_t: [u8; SIZE_T] = next_det_val.try_into().expect(&format!(
            "Expected {SIZE_T} bytes instead of {next_det_val_len} bytes in the following det vals vec"
        ));
        return std::mem::transmute_copy::<[u8; SIZE_T], T>(&bytes_t);
    }

    #[cfg(not(feature = "exe_trace"))]
    #[allow(unreachable_code)]
    any_raw_inner::<T>()
}

/// This low-level function returns nondet bytes of size T.
#[rustc_diagnostic_item = "KaniAnyRaw"]
#[inline(never)]
#[allow(dead_code)]
fn any_raw_inner<T>() -> T {
    unimplemented!("Kani any_raw_inner");
}

/// This function sets deterministic values and plays back the user's proof harness.
#[cfg(feature = "exe_trace")]
pub fn exe_trace_run<F: Fn()>(mut det_vals: Vec<Vec<u8>>, proof_harness: F) {
    // Det vals in the user test case should be in the same order as the order of kani::any() calls.
    // Here, we need to reverse this order because det vals are popped off of the outer Vec,
    // so the chronological first det val should come last.
    det_vals.reverse();
    // If another thread panicked while holding the lock (e.g., because they hit an expected assertion failure), we still want to continue.
    let _guard = match DET_VALS_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    unsafe {
        DET_VALS = det_vals;
    }
    // Since F is a type argument, there should be a direct, static call to proof_harness().
    proof_harness();
}

/// Function used in tests for cases where the condition is not always true.
#[inline(never)]
#[rustc_diagnostic_item = "KaniExpectFail"]
pub fn expect_fail(_cond: bool, _message: &'static str) {
    #[cfg(feature = "exe_trace")]
    assert!(!_cond, "{}", _message);
}

/// Function used to generate panic with a static message as this is the only one currently
/// supported by Kani display.
///
/// During verification this will get replaced by `assert(false)`. For concrete executions, we just
/// invoke the regular `std::panic!()` function. This function is used by our standard library
/// overrides, but not the other way around.
#[inline(never)]
#[rustc_diagnostic_item = "KaniPanic"]
#[doc(hidden)]
pub fn panic(message: &'static str) -> ! {
    panic!("{}", message)
}

/// Kani proc macros must be in a separate crate
pub use kani_macros::*;
