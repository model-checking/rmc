// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Zfunction-contracts

// Test that Kani errors if the target of the proof_for_contract is missing a generic argument that's required to disambiguate between multiple implementations
// or if the generic argument is invalid.
// c.f. https://github.com/model-checking/kani/issues/3773

struct NonZero<T>(T);

impl NonZero<u32> {
    #[kani::requires(self.0.checked_mul(x).is_some())]
    fn unchecked_mul(self, x: u32) -> u32 {
        self.0 * x
    }
}

impl NonZero<i32> {
    #[kani::requires(self.0.checked_mul(x).is_some())]
    fn unchecked_mul(self, x: i32) -> i32 {
        self.0 * x
    }
}

// Errors because there is more than one unchecked_mul implementation
#[kani::proof_for_contract(NonZero::unchecked_mul)]
fn verify_unchecked_mul_ambiguous_path() {
    let x: NonZero<i32> = NonZero(-1);
    x.unchecked_mul(-2);
}

// Errors because the g32 implementation doesn't exist
#[kani::proof_for_contract(NonZero::<g32>::unchecked_mul)]
fn verify_unchecked_mul_invalid_impl() {
    let x: NonZero<i32> = NonZero(-1);
    NonZero::<i32>::unchecked_mul(x, -2);
}
