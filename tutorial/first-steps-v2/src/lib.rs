// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// ANCHOR: code
fn estimate_size(x: u32) -> u32 {
    assert!(x < 4096);

    if x < 256 {
        if x < 128 {
            return 1;
        } else {
            return 3;
        }
    } else if x < 1024 {
        if x > 1022 {
            return 4;
        } else {
            return 5;
        }
    } else {
        if x < 2048 {
            return 7;
        } else {
            return 9;
        }
    }
}
// ANCHOR_END: code

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn it_works() {
        assert_eq!(estimate_size(1024), 7);
    }

    // ANCHOR: proptest
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(10000))]
        #[test]
        fn doesnt_crash(x in 0..4095u32) {
            estimate_size(x);
        }
    }
    // ANCHOR_END: proptest
}

// ANCHOR: kani
#[cfg(kani)]
#[kani::proof]
fn verify_success() {
    let x: u32 = kani::any();
    kani::assume(x < 4096);
    let y = estimate_size(x);
    assert!(y < 10);
}
// ANCHOR_END: kani

#[cfg(kani)]
#[kani::proof]
fn will_fail() {
    let x: u32 = kani::any();
    let y = estimate_size(x);
}
