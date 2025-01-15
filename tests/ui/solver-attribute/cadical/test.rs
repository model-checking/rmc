// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: --enable-unstable --cbmc-args --verbosity 9

//! Checks that `cadical` is a valid argument to `kani::solver`

#[kani::proof]
#[kani::solver(cadical)]
fn check() {
    let mut a = [2, 3, 1];
    a.sort();
    assert_eq!(a[0], 1);
    assert_eq!(a[1], 2);
    assert_eq!(a[2], 3);
}
