// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// This module contains functions useful for checking unsafe memory access.
// For full documentation, see the usage of `kani_core::kani_mem!(std);` in library/kani_core/src/lib.rs

// TODO: This module is currently tightly coupled with CBMC's memory model, and it needs some
//       refactoring to be used with other backends.

#[allow(clippy::crate_in_macro_def)]
#[macro_export]
macro_rules! kani_mem {
    ($core:tt) => {
        use super::kani_intrinsic;
        use $core::ptr::{DynMetadata, NonNull, Pointee};

        /// Check if the pointer is valid for write access according to [crate::mem] conditions 1, 2
        /// and 3.
        ///
        /// Note this function also checks for pointer alignment. Use [self::can_write_unaligned]
        /// if you don't want to fail for unaligned pointers.
        ///
        /// This function does not check if the value stored is valid for the given type. Use
        /// [self::can_dereference] for that.
        ///
        /// This function will panic today if the pointer is not null, and it points to an unallocated or
        /// deallocated memory location. This is an existing Kani limitation.
        /// See <https://github.com/model-checking/kani/issues/2690> for more details.
        #[crate::kani::unstable_feature(
            feature = "mem-predicates",
            issue = 2690,
            reason = "experimental memory predicate API"
        )]
        pub fn can_write<T: ?Sized>(ptr: *mut T) -> bool {
            is_ptr_aligned(ptr) && is_inbounds(ptr)
        }

        /// Check if the pointer is valid for unaligned write access according to [crate::mem] conditions
        /// 1, 2 and 3.
        ///
        /// Note this function succeeds for unaligned pointers. See [self::can_write] if you also
        /// want to check pointer alignment.
        ///
        /// This function will panic today if the pointer is not null, and it points to an unallocated or
        /// deallocated memory location. This is an existing Kani limitation.
        /// See <https://github.com/model-checking/kani/issues/2690> for more details.
        #[crate::kani::unstable_feature(
            feature = "mem-predicates",
            issue = 2690,
            reason = "experimental memory predicate API"
        )]
        pub fn can_write_unaligned<T: ?Sized>(ptr: *const T) -> bool {
            let (thin_ptr, metadata) = ptr.to_raw_parts();
            is_inbounds(ptr)
        }

        /// Checks that pointer `ptr` point to a valid value of type `T`.
        ///
        /// For that, the pointer has to be a valid pointer according to [crate::mem] conditions 1, 2
        /// and 3,
        /// and the value stored must respect the validity invariants for type `T`.
        ///
        /// TODO: Kani should automatically add those checks when a de-reference happens.
        /// <https://github.com/model-checking/kani/issues/2975>
        ///
        /// This function will panic today if the pointer is not null, and it points to an unallocated or
        /// deallocated memory location. This is an existing Kani limitation.
        /// See <https://github.com/model-checking/kani/issues/2690> for more details.
        #[crate::kani::unstable_feature(
            feature = "mem-predicates",
            issue = 2690,
            reason = "experimental memory predicate API"
        )]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub fn can_dereference<T: ?Sized>(ptr: *const T) -> bool {
            // Need to assert `is_initialized` because non-determinism is used under the hood, so it
            // does not make sense to use it inside assumption context.
            is_ptr_aligned(ptr)
                && is_inbounds(ptr)
                && assert_is_initialized(ptr)
                && unsafe { has_valid_value(ptr) }
        }

        /// Checks that pointer `ptr` point to a valid value of type `T`.
        ///
        /// For that, the pointer has to be a valid pointer according to [crate::mem] conditions 1, 2
        /// and 3,
        /// and the value stored must respect the validity invariants for type `T`.
        ///
        /// Note this function succeeds for unaligned pointers. See [self::can_dereference] if you also
        /// want to check pointer alignment.
        ///
        /// This function will panic today if the pointer is not null, and it points to an unallocated or
        /// deallocated memory location. This is an existing Kani limitation.
        /// See <https://github.com/model-checking/kani/issues/2690> for more details.
        #[crate::kani::unstable_feature(
            feature = "mem-predicates",
            issue = 2690,
            reason = "experimental memory predicate API"
        )]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub fn can_read_unaligned<T: ?Sized>(ptr: *const T) -> bool {
            let (thin_ptr, metadata) = ptr.to_raw_parts();
            // Need to assert `is_initialized` because non-determinism is used under the hood, so it
            // does not make sense to use it inside assumption context.
            is_inbounds(ptr) && assert_is_initialized(ptr) && unsafe { has_valid_value(ptr) }
        }

        /// Check if two pointers points to the same allocated object, and that both pointers
        /// are in bounds of that object.
        ///
        /// A pointer is still considered in-bounds if it points to 1-byte past the allocation.
        #[crate::kani::unstable_feature(
            feature = "mem-predicates",
            issue = 2690,
            reason = "experimental memory predicate API"
        )]
        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub fn same_allocation<T: ?Sized>(ptr1: *const T, ptr2: *const T) -> bool {
            same_allocation_internal(ptr1, ptr2)
        }

        #[allow(clippy::not_unsafe_ptr_arg_deref)]
        pub(super) fn same_allocation_internal<T: ?Sized>(ptr1: *const T, ptr2: *const T) -> bool {
            let addr1 = ptr1 as *const ();
            let addr2 = ptr2 as *const ();
            cbmc::same_allocation(addr1, addr2)
        }

        /// Compute the size of the val pointed to if it is safe to do so.
        ///
        /// Return `None` if an overflow would occur, or if alignment is not power of two.
        /// TODO: Optimize this if T is sized.
        #[kanitool::fn_marker = "CheckedSizeOfIntrinsic"]
        pub fn checked_size_of_raw<T: ?Sized>(ptr: *const T) -> Option<usize> {
            #[cfg(not(feature = "concrete_playback"))]
            return kani_intrinsic();

            #[cfg(feature = "concrete_playback")]
            if core::mem::size_of::<<T as Pointee>::Metadata>() == 0 {
                // SAFETY: It is currently safe to call this with a thin pointer.
                unsafe { Some(core::mem::size_of_val_raw(ptr)) }
            } else {
                panic!("Cannot safely compute size of `{}` at runtime", core::any::type_name::<T>())
            }
        }

        /// Compute the size of the val pointed to if safe.
        ///
        /// Return `None` if alignment information cannot be retrieved (foreign types), or if value
        /// is not power-of-two.
        #[kanitool::fn_marker = "CheckedAlignOfIntrinsic"]
        pub fn checked_align_of_raw<T: ?Sized>(ptr: *const T) -> Option<usize> {
            #[cfg(not(feature = "concrete_playback"))]
            return kani_intrinsic();

            #[cfg(feature = "concrete_playback")]
            if core::mem::size_of::<<T as Pointee>::Metadata>() == 0 {
                // SAFETY: It is currently safe to call this with a thin pointer.
                unsafe { Some(core::mem::align_of_val_raw(ptr)) }
            } else {
                panic!("Cannot safely compute size of `{}` at runtime", core::any::type_name::<T>())
            }
        }

        /// Checks that `ptr` points to an allocation that can hold data of size calculated from `T`.
        ///
        /// This will panic if `ptr` points to an invalid `non_null`
        fn is_inbounds<T: ?Sized>(ptr: *const T) -> bool {
            // If size overflows, then pointer cannot be inbounds.
            let Some(sz) = checked_size_of_raw(ptr) else { return false };
            if sz == 0 {
                true // ZST pointers are always valid including nullptr.
            } else if ptr.is_null() {
                false
            } else {
                // Note that this branch can't be tested in concrete execution as `is_read_ok` needs to be
                // stubbed.
                // We first assert that the data_ptr
                let data_ptr = ptr as *const ();
                if !unsafe { is_allocated(data_ptr, 0) } {
                    crate::kani::unsupported(
                        "Kani does not support reasoning about pointer to unallocated memory",
                    );
                }
                unsafe { is_allocated(data_ptr, sz) }
            }
        }

        // Return whether the pointer is aligned
        #[allow(clippy::manual_is_power_of_two)]
        fn is_ptr_aligned<T: ?Sized>(ptr: *const T) -> bool {
            // Cannot be aligned if pointer alignment cannot be computed.
            let Some(align) = checked_align_of_raw(ptr) else { return false };
            if align > 0 && (align & (align - 1)) == 0 {
                // Mod of power of 2 can be done with an &.
                ptr as *const () as usize & (align - 1) == 0
            } else {
                // Alignment is not a valid value (not a power of two).
                false
            }
        }

        /// Check if the pointer `_ptr` contains an allocated address of size equal or greater than `_size`.
        ///
        /// # Safety
        ///
        /// This function should only be called to ensure a pointer is always valid, i.e., in an assertion
        /// context.
        ///
        /// I.e.: This function always returns `true` if the pointer is valid.
        /// Otherwise, it returns non-det boolean.
        #[kanitool::fn_marker = "IsAllocatedHook"]
        #[inline(never)]
        unsafe fn is_allocated(_ptr: *const (), _size: usize) -> bool {
            kani_intrinsic()
        }

        /// Check if the value stored in the given location satisfies type `T` validity requirements.
        ///
        /// # Safety
        ///
        /// - Users have to ensure that the pointed to memory is allocated.
        #[kanitool::fn_marker = "ValidValueIntrinsic"]
        #[inline(never)]
        unsafe fn has_valid_value<T: ?Sized>(_ptr: *const T) -> bool {
            kani_intrinsic()
        }

        /// Check whether `len * size_of::<T>()` bytes are initialized starting from `ptr`.
        #[kanitool::fn_marker = "IsInitializedIntrinsic"]
        #[inline(never)]
        pub(crate) fn is_initialized<T: ?Sized>(_ptr: *const T) -> bool {
            kani_intrinsic()
        }

        /// A helper to assert `is_initialized` to use it as a part of other predicates.
        fn assert_is_initialized<T: ?Sized>(ptr: *const T) -> bool {
            super::internal::check(
                is_initialized(ptr),
                "Undefined Behavior: Reading from an uninitialized pointer",
            );
            true
        }

        pub(super) mod cbmc {
            use super::*;
            /// CBMC specific implementation of [super::same_allocation].
            pub fn same_allocation(addr1: *const (), addr2: *const ()) -> bool {
                let obj1 = crate::kani::mem::pointer_object(addr1);
                (obj1 == crate::kani::mem::pointer_object(addr2)) && {
                    if !unsafe { is_allocated(addr1, 0) || is_allocated(addr2, 0) } {
                        crate::kani::unsupported(
                            "Kani does not support reasoning about pointer to unallocated memory",
                        );
                    }
                    unsafe { is_allocated(addr1, 0) && is_allocated(addr2, 0) }
                }
            }
        }

        /// Get the object ID of the given pointer.
        #[doc(hidden)]
        #[kanitool::fn_marker = "PointerObjectHook"]
        #[inline(never)]
        pub(crate) fn pointer_object<T: ?Sized>(_ptr: *const T) -> usize {
            kani_intrinsic()
        }

        /// Get the object offset of the given pointer.
        #[doc(hidden)]
        #[crate::kani::unstable_feature(
            feature = "ghost-state",
            issue = 3184,
            reason = "experimental ghost state/shadow memory API"
        )]
        #[kanitool::fn_marker = "PointerOffsetHook"]
        #[inline(never)]
        pub fn pointer_offset<T: ?Sized>(_ptr: *const T) -> usize {
            kani_intrinsic()
        }
    };
}
