// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Zfunction-contracts -Zmem-predicates

//! Test that it is sound to use memory predicates inside a contract pre-condition.
//! We cannot validate post-condition yet. This can be done once we fix:
//! <https://github.com/model-checking/kani/issues/2997>

mod pre_condition {
    /// This contract should succeed only if the input is a valid pointer.
    #[kani::requires(kani::mem::can_dereference(ptr))]
    unsafe fn read_ptr(ptr: *const i32) -> i32 {
        *ptr
    }

    #[kani::proof_for_contract(read_ptr)]
    fn harness_head_ptr() {
        let boxed = Box::new(10);
        let raw_ptr = Box::into_raw(boxed);
        assert_eq!(unsafe { read_ptr(raw_ptr) }, 10);
        let _ = unsafe { Box::from_raw(raw_ptr) };
    }

    #[kani::proof_for_contract(read_ptr)]
    fn harness_stack_ptr() {
        let val = -20;
        assert_eq!(unsafe { read_ptr(&val) }, -20);
    }

    #[kani::proof_for_contract(read_ptr)]
    #[kani::should_panic]
    fn harness_invalid_ptr() {
        let ptr = std::ptr::NonNull::<i32>::dangling().as_ptr();
        assert_eq!(unsafe { read_ptr(ptr) }, -20);
    }
}

mod post_condition {
    /// This contract should fail since we are creating a dangling pointer.
    #[kani::ensures(|result| kani::mem::can_dereference((*result).0))]
    unsafe fn new_invalid_ptr() -> PtrWrapper<char> {
        let var = 'c';
        PtrWrapper(&var as *const _)
    }

    #[kani::proof_for_contract(new_invalid_ptr)]
    fn harness() {
        let _ = unsafe { new_invalid_ptr() };
    }

    struct PtrWrapper<T>(*const T);

    impl<T> kani::Arbitrary for PtrWrapper<T> {
        fn any() -> Self {
            unreachable!("Do not invoke stubbing")
        }
    }
}
