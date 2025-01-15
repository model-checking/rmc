// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: --solver cadical --enable-unstable --cbmc-args --verbosity 9

//! Checks that the `cadical` is supported as an argument to `--solver`

#[kani::proof]
fn check_solver_option() {
    let v = vec![kani::any(), 2];
    let v_copy = v.clone();
    assert_eq!(v, v_copy);
}
