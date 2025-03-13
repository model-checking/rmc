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
mod float;
mod mem;
mod mem_init;
mod models;

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
            kani_core::generate_models!();

            pub mod float {
                kani_core::generate_float!(core);
            }

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
        kani_core::generate_models!();

        pub mod float {
            //! This module contains functions useful for float-related checks
            kani_core::generate_float!(std);
        }

        pub mod mem {
            //! This module contains functions useful for checking unsafe memory access.
            //!
            //! Given the following validity rules provided in the Rust documentation:
            //! <https://doc.rust-lang.org/std/ptr/index.html> (accessed Feb 6th, 2024)
            //!
            //! 1. A null pointer is never valid, not even for accesses of size zero.
            //! 2. For a pointer to be valid, it is necessary, but not always sufficient, that the pointer
            //!    be dereferenceable: the memory range of the given size starting at the pointer must all be
            //!    within the bounds of a single allocated object. Note that in Rust, every (stack-allocated)
            //!    variable is considered a separate allocated object.
            //!    ~~Even for operations of size zero, the pointer must not be pointing to deallocated memory,
            //!    i.e., deallocation makes pointers invalid even for zero-sized operations.~~
            //!    ZST access is not OK for any pointer.
            //!    See: <https://github.com/rust-lang/unsafe-code-guidelines/issues/472>
            //! 3. However, casting any non-zero integer literal to a pointer is valid for zero-sized
            //!    accesses, even if some memory happens to exist at that address and gets deallocated.
            //!    This corresponds to writing your own allocator: allocating zero-sized objects is not very
            //!    hard. The canonical way to obtain a pointer that is valid for zero-sized accesses is
            //!    `NonNull::dangling`.
            //! 4. All accesses performed by functions in this module are non-atomic in the sense of atomic
            //!    operations used to synchronize between threads.
            //!    This means it is undefined behavior to perform two concurrent accesses to the same location
            //!    from different threads unless both accesses only read from memory.
            //!    Notice that this explicitly includes `read_volatile` and `write_volatile`:
            //!    Volatile accesses cannot be used for inter-thread synchronization.
            //! 5. The result of casting a reference to a pointer is valid for as long as the underlying
            //!    object is live and no reference (just raw pointers) is used to access the same memory.
            //!    That is, reference and pointer accesses cannot be interleaved.
            //!
            //! Kani is able to verify #1 and #2 today.
            //!
            //! For #3, we are overly cautious, and Kani will only consider zero-sized pointer access safe if
            //! the address matches `NonNull::<()>::dangling()`.
            //! The way Kani tracks provenance is not enough to check if the address was the result of a cast
            //! from a non-zero integer literal.
            //!
            kani_core::kani_mem!(std);
        }

        mod mem_init {
            //! This module provides instrumentation for tracking memory initialization of raw pointers.
            //!
            //! Currently, memory initialization is tracked on per-byte basis, so each byte of memory pointed to
            //! by raw pointers could be either initialized or uninitialized. Padding bytes are always
            //! considered uninitialized when read as data bytes. Each type has a type layout to specify which
            //! bytes are considered to be data and which -- padding. This is determined at compile time and
            //! statically injected into the program (see `Layout`).
            //!
            //! Compiler automatically inserts calls to `is_xxx_initialized` and `set_xxx_initialized` at
            //! appropriate locations to get or set the initialization status of the memory pointed to.
            //!
            //! Note that for each harness, tracked object and tracked offset are chosen non-deterministically,
            //! so calls to `is_xxx_initialized` should be only used in assertion contexts.
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
        #[kanitool::fn_marker = "AssumeHook"]
        #[cfg(not(feature = "concrete_playback"))]
        pub fn assume(cond: bool) {
            let _ = cond;
        }

        #[inline(never)]
        #[kanitool::fn_marker = "AssumeHook"]
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
        #[kanitool::fn_marker = "AssertHook"]
        pub const fn assert(cond: bool, msg: &'static str) {
            let _ = cond;
            let _ = msg;
        }

        #[cfg(feature = "concrete_playback")]
        #[inline(never)]
        #[kanitool::fn_marker = "AssertHook"]
        pub const fn assert(cond: bool, msg: &'static str) {
            assert!(cond, "{}", msg);
        }

        #[macro_export]
        macro_rules! forall {
            (|$i:ident in ($lower_bound:expr, $upper_bound:expr)| $predicate:expr) => {{
                let lower_bound: usize = $lower_bound;
                let upper_bound: usize = $upper_bound;
                let predicate = |$i| $predicate;
                kani_forall(lower_bound, upper_bound, predicate)
            }};
        }

        #[macro_export]
        macro_rules! exists {
            (|$i:ident in ($lower_bound:expr, $upper_bound:expr)| $predicate:expr) => {{
                let lower_bound: usize = $lower_bound;
                let upper_bound: usize = $upper_bound;
                let predicate = |$i| $predicate;
                kani_exists(lower_bound, upper_bound, predicate)
            }};
        }

        #[inline(never)]
        #[kanitool::fn_marker = "ForallHook"]
        pub fn kani_forall<T, F>(lower_bound: T, upper_bound: T, predicate: F) -> bool
        where
            F: Fn(T) -> bool,
        {
            predicate(lower_bound)
        }

        #[inline(never)]
        #[kanitool::fn_marker = "ExistsHook"]
        pub fn kani_exists<T, F>(lower_bound: T, upper_bound: T, predicate: F) -> bool
        where
            F: Fn(T) -> bool,
        {
            predicate(lower_bound)
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
        #[kanitool::fn_marker = "CoverHook"]
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
        #[kanitool::fn_marker = "AnyModel"]
        #[inline(always)]
        pub fn any<T: Arbitrary>() -> T {
            T::any()
        }

        /// This function is only used for function contract instrumentation.
        /// It behaves exaclty like `kani::any<T>()`, except it will check for the trait bounds
        /// at compilation time. It allows us to avoid type checking errors while using function
        /// contracts only for verification.
        #[kanitool::fn_marker = "AnyModifiesIntrinsic"]
        #[inline(never)]
        #[doc(hidden)]
        pub fn any_modifies<T>() -> T {
            // This function should not be reacheable.
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
        #[kanitool::fn_marker = "AnyRawHook"]
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
        #[kanitool::fn_marker = "PanicHook"]
        #[doc(hidden)]
        pub const fn panic(message: &'static str) -> ! {
            panic!("{}", message)
        }

        #[doc(hidden)]
        #[allow(dead_code)]
        #[kanitool::fn_marker = "SafetyCheckHook"]
        #[inline(never)]
        pub(crate) fn safety_check(cond: bool, msg: &'static str) {
            #[cfg(not(feature = "concrete_playback"))]
            return kani_intrinsic();

            #[cfg(feature = "concrete_playback")]
            assert!(cond, "Safety check failed: {msg}");
        }

        #[doc(hidden)]
        #[allow(dead_code)]
        #[kanitool::fn_marker = "SafetyCheckNoAssumeHook"]
        #[inline(never)]
        pub(crate) fn safety_check_no_assume(cond: bool, msg: &'static str) {
            #[cfg(not(feature = "concrete_playback"))]
            return kani_intrinsic();

            #[cfg(feature = "concrete_playback")]
            assert!(cond, "Safety check failed: {msg}");
        }

        /// This should indicate that Kani does not support a certain operation.
        #[doc(hidden)]
        #[allow(dead_code)]
        #[kanitool::fn_marker = "UnsupportedCheckHook"]
        #[inline(never)]
        #[allow(clippy::diverging_sub_expression)]
        pub(crate) fn unsupported(msg: &'static str) -> ! {
            #[cfg(not(feature = "concrete_playback"))]
            return kani_intrinsic();

            #[cfg(feature = "concrete_playback")]
            unimplemented!("Unsupported Kani operation: {msg}")
        }

        /// An empty body that can be used to define Kani intrinsic functions.
        ///
        /// A Kani intrinsic is a function that is interpreted by Kani compiler.
        /// While we could use `unreachable!()` or `panic!()` as the body of a kani intrinsic
        /// function, both cause Kani to produce a warning since we don't support caller location.
        /// (see https://github.com/model-checking/kani/issues/2010).
        ///
        /// This function is dead, since its caller is always  handled via a hook anyway,
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

            /// Used to hold the bodies of automatically generated harnesses.
            #[kanitool::fn_marker = "AutomaticHarnessIntrinsic"]
            pub fn automatic_harness() {
                super::kani_intrinsic()
            }

            /// A way to break the ownerhip rules. Only used by contracts where we can
            /// guarantee it is done safely.
            #[inline(never)]
            #[doc(hidden)]
            #[kanitool::fn_marker = "UntrackedDerefHook"]
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
            #[kanitool::fn_marker = "InitContractsHook"]
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
            #[kanitool::fn_marker = "WriteAnyIntrinsic"]
            #[inline(never)]
            #[doc(hidden)]
            pub unsafe fn write_any<T: ?Sized>(_pointer: *mut T) {
                // This function should not be reacheable.
                // Users must include `#[kani::recursion]` in any function contracts for recursive functions;
                // otherwise, this might not be properly instantiate. We mark this as unreachable to make
                // sure Kani doesn't report any false positives.
                super::kani_intrinsic()
            }

            /// Fill in a slice with kani::any.
            /// Intended as a post compilation replacement for write_any
            #[kanitool::fn_marker = "WriteAnySliceModel"]
            #[inline(always)]
            pub unsafe fn write_any_slice<T: Arbitrary>(slice: *mut [T]) {
                (*slice).fill_with(T::any)
            }

            /// Fill in a pointer with kani::any.
            /// Intended as a post compilation replacement for write_any
            #[kanitool::fn_marker = "WriteAnySlimModel"]
            #[inline(always)]
            pub unsafe fn write_any_slim<T: Arbitrary>(pointer: *mut T) {
                ptr::write(pointer, T::any())
            }

            /// Fill in a str with kani::any.
            /// Intended as a post compilation replacement for write_any.
            /// Not yet implemented
            #[kanitool::fn_marker = "WriteAnyStrModel"]
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
            #[kanitool::fn_marker = "RunContractModel"]
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
            #[kanitool::fn_marker = "RunLoopContractModel"]
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

            /// Insert the contract into the body of the function as assertion(s).
            pub const ASSERT: Mode = 4;

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
            #[kanitool::fn_marker = "CheckHook"]
            pub(crate) const fn check(cond: bool, msg: &'static str) {
                let _ = cond;
                let _ = msg;
            }

            #[cfg(feature = "concrete_playback")]
            #[inline(never)]
            #[kanitool::fn_marker = "CheckHook"]
            pub(crate) const fn check(cond: bool, msg: &'static str) {
                assert!(cond, "{}", msg);
            }
        }
    };
}
