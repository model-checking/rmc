// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Zfunction-contracts

// Test `cover_contract` functionality, which fails verification for a unsatisfiable precondition
// or unreachable postcondition.
// See https://github.com/model-checking/kani/issues/2793

// Test that verification fails for an unreachable postcondition.

// The precondition is satisfiable, but the postcondition is unreachable because the code panics.
// Test that in the case where the postcondition check's property status is unreachable (because the function panics)
// we change the status to failure.
#[kani::requires(a > 5)]
#[kani::ensures(|result: &u32| *result == a)]
fn unreachable_postcondition(a: u32) -> u32 {
    panic!("panic")
}

#[kani::proof_for_contract(unreachable_postcondition)]
fn prove_unreachable_postcondition() {
    let x: u32 = kani::any();
    unreachable_postcondition(x);
}

// Unreachable postcondition because the function never returns.
// Test that in the case where the postcondition check's property status is undetermined because of unwinding failures,
// we change the status to failure.
#[kani::ensures(|result: &u32| *result == 7)]
fn never_return() -> u32 {
    loop {}
    7
}

#[kani::proof_for_contract(never_return)]
#[kani::unwind(5)]
fn prove_never_return() {
    never_return();
}
