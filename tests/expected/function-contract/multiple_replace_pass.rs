// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: -Zfunction-contracts

#[kani::ensures(|result : &u32| *result == 1)]
fn one() -> u32 {
    1
}

#[kani::proof_for_contract(one)]
fn check_one() {
    let _ = one();
}

#[kani::ensures(|result : &u32| *result == 1)]
fn one_too() -> u32 {
    1
}

#[kani::proof_for_contract(one_too)]
fn check_one_too() {
    let _ = one_too();
}

#[kani::proof]
#[kani::stub_verified(one)]
#[kani::stub_verified(one_too)]
fn main() {
    assert_eq!(one(), one_too());
}
