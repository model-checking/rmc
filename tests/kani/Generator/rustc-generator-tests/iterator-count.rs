// Copyright rustc Contributors
// Adapted from rustc: https://github.com/rust-lang/rust/tree/5f98537eb7b5f42c246a52c550813c3cff336069/src/test/ui/generator/iterator-count.rs
//
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Modifications Copyright Kani Contributors
// See GitHub history for details.

// run-pass

#![feature(generators, generator_trait)]

use std::marker::Unpin;
use std::ops::{Generator, GeneratorState};
use std::pin::Pin;

struct W<T>(T);

// This impl isn't safe in general, but the generator used in this test is movable
// so it won't cause problems.
impl<T: Generator<(), Return = ()> + Unpin> Iterator for W<T> {
    type Item = T::Yield;

    fn next(&mut self) -> Option<Self::Item> {
        match Pin::new(&mut self.0).resume(()) {
            GeneratorState::Complete(..) => None,
            GeneratorState::Yielded(v) => Some(v),
        }
    }
}

fn test() -> impl Generator<(), Return = (), Yield = u8> + Unpin {
    || {
        for i in 1..6 {
            yield i
        }
    }
}

#[kani::proof]
#[kani::unwind(11)]
fn main() {
    let end = 11;

    let closure_test = |start| {
        move || {
            for i in start..end {
                yield i
            }
        }
    };

    assert!(W(test()).chain(W(closure_test(6))).eq(1..11));
}
