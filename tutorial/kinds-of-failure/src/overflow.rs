// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-verify-fail

// ANCHOR: code
fn simple_addition(a: u32, b: u32) -> u32 {
    return a + b;
}
// ANCHOR_END: code

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ANCHOR: proptest
    proptest! {
        #[test]
        fn doesnt_crash(a: u32, b: u32) {
            simple_addition(a, b);
        }
    }
    // ANCHOR_END: proptest
}

// ANCHOR: kani
#[cfg(kani)]
#[kani::proof]
fn add_overflow() {
    let a: u32 = kani::any();
    let b: u32 = kani::any();
    simple_addition(a, b);
}
// ANCHOR_END: kani
