// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Zfunction-contracts

// Test `cover_contract` functionality, which fails verification for a unsatisfiable precondition
// or unreachable postcondition.
// See https://github.com/model-checking/kani/issues/2793

// Undetermined whether we can reach the postcondition because we encounter an unsupported construct.

#[kani::proof_for_contract(unsupp)]
fn undetermined_example() {
    let mut x = 0;
    unsupp(&mut x);
    assert!(x == 0);
}

#[kani::requires(true)]
#[kani::ensures(|res| *res == *x)]
fn unsupp(x: &mut u8) -> u8 {
    unsafe {
        std::arch::asm!("nop");
    }
    *x
}
