// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

fn get(s: &[i16], index: usize) -> i16 {
    s[index]
}

#[kani::proof]
fn main() {
    get(&[7, -83, 19], 15);
}
