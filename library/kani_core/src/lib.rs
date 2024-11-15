// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! This crate is a macro_only crate. It is designed to be used in `no_core` and `no_std`
//! environment.
//!
//! It will contain macros that generate core components of Kani.
//!
//! For regular usage, the kani library will invoke these macros to generate its components as if
//! they were declared in that library.
//!
//! For `no_core` and `no_std` crates, they will have to directly invoke those macros inside a
//! `kani` module in order to generate all the required components.
//! I.e., the components will be part of the crate being compiled.
//!
//! Note that any crate level attribute should be added by kani_driver as RUSTC_FLAGS.
//! E.g.: `register_tool(kanitool)`

#![feature(no_core)]
#![no_core]
#![feature(f16)]
#![feature(f128)]

mod arbitrary;
mod mem;
mod mem_init;

pub use kani_macros::*;

/// Users should only need to invoke this.
///
/// Options are:
/// - `kani`: Add definitions needed for Kani library.
/// - `core`: Define a `kani` module inside `core` crate.
/// - `std`: TODO: Define a `kani` module inside `std` crate. Users must define kani inside core.
#[macro_export]
macro_rules! kani_lib {
    (core) => {
        #[cfg(kani)]
        #[unstable(feature = "kani", issue = "none")]
        pub mod kani {
            // We need to list them all today because there is conflict with unstable.
            pub use kani_core::*;
            kani_core::kani_intrinsics!(core);
            kani_core::generate_arbitrary!(core);

            pub mod mem {
                kani_core::kani_mem!(core);
            }

            mod mem_init {
                kani_core::kani_mem_init!(core);
            }
        }
    };

    (kani) => {
        pub use kani_core::*;
        kani_core::kani_intrinsics!(std);
        kani_core::generate_arbitrary!(std);

        pub mod mem {
            kani_core::kani_mem!(std);
        }

        mod mem_init {
            kani_core::kani_mem_init!(std);
        }
    };
}

/// Kani intrinsics contains the public APIs used by users to verify their harnesses.
/// This macro is a part of kani_core as that allows us to verify even libraries that are no_core
/// such as core in rust's std library itself.
///
/// TODO: Use this inside kani library so that we dont have to maintain two copies of the same intrinsics.
#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! kani_intrinsics {
    ($core:tt) => {
        /// Creates an assumption that will be valid after this statement run. Note that the assumption
        /// will only be applied for paths that follow the assumption. If the assumption doesn't hold, the
        /// program will exit successfully.
        ///
        /// # Example:
        ///
        /// The code snippet below should never panic.
        ///
        /// ```no_run
        /// let i : i32 = kani::any();
        /// kani::assume(i > 10);
        /// if i < 0 {
        ///   panic!("This will never panic");
        /// }
        /// ```
        ///
        /// The following code may panic though:
        ///
        /// ```no_run
        /// let i : i32 = kani::any();
        /// assert!(i < 0, "This may panic and verification should fail.");
        /// kani::assume(i > 10);
        /// ```
        #[inline(never)]
        #[rustc_diagnostic_item = "KaniAssume"]
        #[cfg(not(feature = "concrete_playback"))]
        pub fn assume(cond: bool) {
            let _ = cond;
        }

        #[inline(never)]
        #[rustc_diagnostic_item = "KaniAssume"]
        #[cfg(feature = "concrete_playback")]
        pub fn assume(cond: bool) {
            assert!(cond, "`kani::assume` should always hold");
        }

        /// Creates an assertion of the specified condition and message.
        ///
        /// # Example:
        ///
        /// ```no_run
        /// let x: bool = kani::any();
        /// let y = !x;
        /// kani::assert(x || y, "ORing a boolean variable with its negation must be true")
        /// ```
        #[cfg(not(feature = "concrete_playback"))]
        #[inline(never)]
        #[rustc_diagnostic_item = "KaniAssert"]
        pub const fn assert(cond: bool, msg: &'static str) {
            let _ = cond;
            let _ = msg;
        }

        #[cfg(feature = "concrete_playback")]
        #[inline(never)]
        #[rustc_diagnostic_item = "KaniAssert"]
        pub const fn assert(cond: bool, msg: &'static str) {
            assert!(cond, "{}", msg);
        }

        /// Creates a cover property with the specified condition and message.
        ///
        /// # Example:
        ///
        /// ```no_run
        /// # use crate::kani;
        /// #
        /// # let array: [u8; 10]  = kani::any();
        /// # let slice = kani::slice::any_slice_of_array(&array);
        /// kani::cover(slice.len() == 0, "The slice may have a length of 0");
        /// ```
        ///
        /// A cover property checks if there is at least one execution that satisfies
        /// the specified condition at the location in which the function is called.
        ///
        /// Cover properties are reported as:
        ///  - SATISFIED: if Kani found an execution that satisfies the condition
        ///  - UNSATISFIABLE: if Kani proved that the condition cannot be satisfied
        ///  - UNREACHABLE: if Kani proved that the cover property itself is unreachable (i.e. it is vacuously UNSATISFIABLE)
        ///
        /// This function is called by the [`cover!`] macro. The macro is more
        /// convenient to use.
        ///
        #[inline(never)]
        #[rustc_diagnostic_item = "KaniCover"]
        pub const fn cover(_cond: bool, _msg: &'static str) {}

        /// This creates an symbolic *valid* value of type `T`. You can assign the return value of this
        /// function to a variable that you want to make symbolic.
        ///
        /// # Example:
        ///
        /// In the snippet below, we are verifying the behavior of the function `fn_under_verification`
        /// under all possible `NonZeroU8` input values, i.e., all possible `u8` values except zero.
        ///
        /// ```no_run
        /// # use std::num::NonZeroU8;
        /// # use crate::kani;
        /// #
        /// # fn fn_under_verification(_: NonZeroU8) {}
        /// let inputA = kani::any::<core::num::NonZeroU8>();
        /// fn_under_verification(inputA);
        /// ```
        ///
        /// Note: This is a safe construct and can only be used with types that implement the `Arbitrary`
        /// trait. The Arbitrary trait is used to build a symbolic value that represents all possible
        /// valid values for type `T`.
        #[rustc_diagnostic_item = "KaniAny"]
        #[inline(always)]
        pub fn any<T: Arbitrary>() -> T {
            T::any()
        }

        /// This function is only used for function contract instrumentation.
        /// It behaves exaclty like `kani::any<T>()`, except it will check for the trait bounds
        /// at compilation time. It allows us to avoid type checking errors while using function
        /// contracts only for verification.
        #[rustc_diagnostic_item = "KaniAnyModifies"]
        #[inline(never)]
        #[doc(hidden)]
        pub fn any_modifies<T>() -> T {
            // This function should not be reachable.
            // Users must include `#[kani::recursion]` in any function contracts for recursive functions;
            // otherwise, this might not be properly instantiate. We mark this as unreachable to make
            // sure Kani doesn't report any false positives.
            unreachable!()
        }

        /// This creates a symbolic *valid* value of type `T`.
        /// The value is constrained to be a value accepted by the predicate passed to the filter.
        /// You can assign the return value of this function to a variable that you want to make symbolic.
        ///
        /// # Example:
        ///
        /// In the snippet below, we are verifying the behavior of the function `fn_under_verification`
        /// under all possible `u8` input values between 0 and 12.
        ///
        /// ```no_run
        /// # use std::num::NonZeroU8;
        /// # use crate::kani;
        /// #
        /// # fn fn_under_verification(_: u8) {}
        /// let inputA: u8 = kani::any_where(|x| *x < 12);
        /// fn_under_verification(inputA);
        /// ```
        ///
        /// Note: This is a safe construct and can only be used with types that implement the `Arbitrary`
        /// trait. The Arbitrary trait is used to build a symbolic value that represents all possible
        /// valid values for type `T`.
        #[inline(always)]
        pub fn any_where<T: Arbitrary, F: FnOnce(&T) -> bool>(f: F) -> T {
            let result = T::any();
            assume(f(&result));
            result
        }

        /// This function creates a symbolic value of type `T`. This may result in an invalid value.
        ///
        /// # Safety
        ///
        /// This function is unsafe and it may represent invalid `T` values which can lead to many
        /// undesirable undefined behaviors. Because of that, this function can only be used
        /// internally when we can guarantee that the type T has no restriction regarding its bit level
        /// representation.
        ///
        /// This function is also used to find concrete values in the CBMC output trace
        /// and return those concrete values in concrete playback mode.
        ///
        /// Note that SIZE_T must be equal the size of type T in bytes.
        #[inline(never)]
        #[cfg(not(feature = "concrete_playback"))]
        unsafe fn any_raw_internal<T: Copy>() -> T {
            any_raw::<T>()
        }

        /// This is the same as [any_raw_internal] for verification flow, but not for concrete playback.
        #[inline(never)]
        #[cfg(not(feature = "concrete_playback"))]
        unsafe fn any_raw_array<T: Copy, const N: usize>() -> [T; N] {
            any_raw::<[T; N]>()
        }

        #[cfg(feature = "concrete_playback")]
        use concrete_playback::{any_raw_array, any_raw_internal};

        /// This low-level function returns nondet bytes of size T.
        #[rustc_diagnostic_item = "KaniAnyRaw"]
        #[inline(never)]
        #[allow(dead_code)]
        fn any_raw<T: Copy>() -> T {
            kani_intrinsic()
        }

        /// Function used to generate panic with a static message as this is the only one currently
        /// supported by Kani display.
        ///
        /// During verification this will get replaced by `assert(false)`. For concrete executions, we just
        /// invoke the regular `core::panic!()` function. This function is used by our standard library
        /// overrides, but not the other way around.
        #[inline(never)]
        #[rustc_diagnostic_item = "KaniPanic"]
        #[doc(hidden)]
        pub const fn panic(message: &'static str) -> ! {
            panic!("{}", message)
        }

        /// An empty body that can be used to define Kani intrinsic functions.
        ///
        /// A Kani intrinsic is a function that is interpreted by Kani compiler.
        /// While we could use `unreachable!()` or `panic!()` as the body of a kani intrinsic
        /// function, both cause Kani to produce a warning since we don't support caller location.
        /// (see https://github.com/model-checking/kani/issues/2010).
        ///
        /// This function is dead, since its caller is always handled via a hook anyway,
        /// so we just need to put a body that rustc does not complain about.
        /// An infinite loop works out nicely.
        fn kani_intrinsic<T>() -> T {
            #[allow(clippy::empty_loop)]
            loop {}
        }

        #[doc(hidden)]
        pub mod internal {
            use crate::kani::Arbitrary;
            use core::ptr;

            /// Helper trait for code generation for `modifies` contracts.
            ///
            /// We allow the user to provide us with a pointer-like object that we convert as needed.
            #[doc(hidden)]
            pub trait Pointer {
                /// Type of the pointed-to data
                type Inner: ?Sized;

                /// used for havocking on replecement of a `modifies` clause.
                unsafe fn assignable(self) -> *mut Self::Inner;
            }

            impl<T: ?Sized> Pointer for &T {
                type Inner = T;
                unsafe fn assignable(self) -> *mut Self::Inner {
                    self as *const T as *mut T
                }
            }

            impl<T: ?Sized> Pointer for &mut T {
                type Inner = T;

                unsafe fn assignable(self) -> *mut Self::Inner {
                    self as *mut T
                }
            }

            impl<T: ?Sized> Pointer for *const T {
                type Inner = T;

                unsafe fn assignable(self) -> *mut Self::Inner {
                    self as *mut T
                }
            }

            impl<T: ?Sized> Pointer for *mut T {
                type Inner = T;
                unsafe fn assignable(self) -> *mut Self::Inner {
                    self
                }
            }

            /// This function is only used for function contract instrumentation.
            /// It is the same as cover(), but if the cover statement is unreachable, it fails the contract harness.
            /// See the contracts module in kani_macros for details.
            #[inline(never)]
            #[rustc_diagnostic_item = "KaniContractCover"]
            #[doc(hidden)]
            pub const fn contract_cover(_cond: bool, _msg: &'static str) {}

            /// A way to break the ownerhip rules. Only used by contracts where we can
            /// guarantee it is done safely.
            #[inline(never)]
            #[doc(hidden)]
            #[rustc_diagnostic_item = "KaniUntrackedDeref"]
            pub fn untracked_deref<T>(_: &T) -> T {
                todo!()
            }

            /// CBMC contracts currently has a limitation where `free` has to be in scope.
            /// However, if there is no dynamic allocation in the harness, slicing removes `free` from the
            /// scope.
            ///
            /// Thus, this function will basically translate into:
            /// ```c
            /// // This is a no-op.
            /// free(NULL);
            /// ```
            #[inline(never)]
            #[doc(hidden)]
            #[rustc_diagnostic_item = "KaniInitContracts"]
            pub fn init_contracts() {}

            /// This should only be used within contracts. The intent is to
            /// perform type inference on a closure's argument
            #[doc(hidden)]
            pub fn apply_closure<T, U: Fn(&T) -> bool>(f: U, x: &T) -> bool {
                f(x)
            }

            /// Recieves a reference to a pointer-like object and assigns kani::any_modifies to that object.
            /// Only for use within function contracts and will not be replaced if the recursive or function stub
            /// replace contracts are not used.
            #[rustc_diagnostic_item = "KaniWriteAny"]
            #[inline(never)]
            #[doc(hidden)]
            pub unsafe fn write_any<T: ?Sized>(_pointer: *mut T) {
                // This function should not be reachable.
                // Users must include `#[kani::recursion]` in any function contracts for recursive functions;
                // otherwise, this might not be properly instantiate. We mark this as unreachable to make
                // sure Kani doesn't report any false positives.
                unreachable!()
            }

            /// Fill in a slice with kani::any.
            /// Intended as a post compilation replacement for write_any
            #[rustc_diagnostic_item = "KaniWriteAnySlice"]
            #[inline(always)]
            pub unsafe fn write_any_slice<T: Arbitrary>(slice: *mut [T]) {
                (*slice).fill_with(T::any)
            }

            /// Fill in a pointer with kani::any.
            /// Intended as a post compilation replacement for write_any
            #[rustc_diagnostic_item = "KaniWriteAnySlim"]
            #[inline(always)]
            pub unsafe fn write_any_slim<T: Arbitrary>(pointer: *mut T) {
                ptr::write(pointer, T::any())
            }

            /// Fill in a str with kani::any.
            /// Intended as a post compilation replacement for write_any.
            /// Not yet implemented
            #[rustc_diagnostic_item = "KaniWriteAnyStr"]
            #[inline(always)]
            pub unsafe fn write_any_str(_s: *mut str) {
                //TODO: strings introduce new UB
                //(*s).as_bytes_mut().fill_with(u8::any)
                //TODO: String validation
                unimplemented!("Kani does not support creating arbitrary `str`")
            }

            /// Function that calls a closure used to implement contracts.
            ///
            /// In contracts, we cannot invoke the generated closures directly, instead, we call register
            /// contract. This function is a no-op. However, in the reality, we do want to call the closure,
            /// so we swap the register body by this function body.
            #[doc(hidden)]
            #[allow(dead_code)]
            #[rustc_diagnostic_item = "KaniRunContract"]
            fn run_contract_fn<T, F: FnOnce() -> T>(func: F) -> T {
                func()
            }

            /// Function that calls a closure used to implement loop contracts.
            ///
            /// In contracts, we cannot invoke the generated closures directly, instead, we call register
            /// contract. This function is a no-op. However, in the reality, we do want to call the closure,
            /// so we swap the register body by this function body.
            #[doc(hidden)]
            #[allow(dead_code)]
            #[rustc_diagnostic_item = "KaniRunLoopContract"]
            fn run_loop_contract_fn<F: Fn() -> bool>(func: &F, _transformed: usize) -> bool {
                func()
            }

            /// This is used by contracts to select which version of the contract to use during codegen.
            #[doc(hidden)]
            pub type Mode = u8;

            /// Keep the original body.
            pub const ORIGINAL: Mode = 0;

            /// Run the check with recursion support.
            pub const RECURSION_CHECK: Mode = 1;

            /// Run the simple check with no recursion support.
            pub const SIMPLE_CHECK: Mode = 2;

            /// Stub the body with its contract.
            pub const REPLACE: Mode = 3;

            /// Creates a non-fatal property with the specified condition and message.
            ///
            /// This check will not impact the program control flow even when it fails.
            ///
            /// # Example:
            ///
            /// ```no_run
            /// let x: bool = kani::any();
            /// let y = !x;
            /// kani::check(x || y, "ORing a boolean variable with its negation must be true");
            /// kani::check(x == y, "A boolean variable is always different than its negation");
            /// kani::cover!(true, "This should still be reachable");
            /// ```
            ///
            #[cfg(not(feature = "concrete_playback"))]
            #[inline(never)]
            #[rustc_diagnostic_item = "KaniCheck"]
            pub(crate) const fn check(cond: bool, msg: &'static str) {
                let _ = cond;
                let _ = msg;
            }

            #[cfg(feature = "concrete_playback")]
            #[inline(never)]
            #[rustc_diagnostic_item = "KaniCheck"]
            pub(crate) const fn check(cond: bool, msg: &'static str) {
                assert!(cond, "{}", msg);
            }
        }
    };
}
