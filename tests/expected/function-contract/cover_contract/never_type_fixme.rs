// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Zfunction-contracts

// Unreachable postcondition because the function never returns.
// Kani should fail verification because the postcondition is unreachable,
// but currently doesn't generate the postcondition check at all
// (although verification still fails because of the unwinding error).
// We may need special never type detection for this case.

#![feature(never_type)]

#[kani::requires(true)]
#[kani::ensures(|result: &!| true)]
fn never_return() -> ! {
    let x = 7;
    loop {}
}

// Unreachable postcondition because the function never returns
#[kani::proof_for_contract(never_return)]
#[kani::unwind(5)]
fn prove_never_return() {
    never_return();
}
