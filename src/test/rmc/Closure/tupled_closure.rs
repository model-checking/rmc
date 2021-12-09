// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Tests that we cover closures with tuples correctly
//! https://github.com/model-checking/rmc/issues/678

fn h(x: u8, y: usize, o: Option<std::num::NonZeroUsize>) -> usize {
    x as usize + y
}

struct Foo {}

impl Foo {
    fn f(&self) -> usize {
        self.g(h)
    }
    fn g<F: Fn(u8, usize, Option<std::num::NonZeroUsize>) -> usize>(&self, ff: F) -> usize {
        ff(5, 22, None)
    }
}

fn main() {
    let x = Foo {};
    assert!(x.f() == 27);
}
