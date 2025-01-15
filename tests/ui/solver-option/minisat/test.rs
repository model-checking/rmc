// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
// kani-flags: --solver minisat --enable-unstable --cbmc-args --verbosity 9

//! Checks that `--solver minisat` is accepted

#[kani::proof]
fn check_solver_option_minisat() {
    let x: i32 = kani::any();
    let y: i32 = kani::any();
    kani::cover!(x == y && x == -789);
}
