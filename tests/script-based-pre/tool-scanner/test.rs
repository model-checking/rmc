// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
//! Sanity check for the utility tool `scanner`.

pub fn check_outer_coercion() {
    assert!(false);
}

unsafe fn do_nothing() {}

pub fn generic<T: Default>() -> T {
    unsafe { do_nothing() };
    T::default()
}

pub struct RecursiveType {
    pub inner: Option<*const RecursiveType>,
}

pub enum RecursiveEnum {
    Base,
    Recursion(Box<RecursiveEnum>),
    RefCell(std::cell::RefCell<f32>),
}

pub fn recursive_type(input1: RecursiveType, input2: RecursiveEnum) {
    let _ = (input1, input2);
}

pub fn with_iterator(input: &[usize]) -> usize {
    input
        .iter()
        .copied()
        .find(|e| *e == 0)
        .unwrap_or_else(|| input.iter().fold(0, |acc, _| acc + 1))
}

pub fn with_for_loop(input: &[usize]) -> usize {
    let mut res = 0;
    for _ in input {
        res += 1;
    }
    res
}

pub fn with_while_loop(input: &[usize]) -> usize {
    let mut res = 0;
    while res < input.len() {
        res += 1;
    }
    return res;
}

pub fn with_loop_loop(input: &[usize]) -> usize {
    let mut res = 0;
    loop {
        if res == input.len() {
            break;
        }
        res += 1;
    }
    res
}

static mut COUNTER: Option<usize> = Some(0);
static OK: bool = true;

pub unsafe fn next_id() -> usize {
    let sum = COUNTER.unwrap() + 1;
    COUNTER = Some(sum);
    sum
}

pub unsafe fn current_id() -> usize {
    COUNTER.unwrap()
}

pub fn ok() -> bool {
    OK
}

pub unsafe fn raw_to_ref<'a, T>(raw: *const T) -> &'a T {
    &*raw
}

pub fn recursion_begin(stop: bool) {
    if !stop {
        recursion_tail()
    }
}

pub fn recursion_tail() {
    recursion_begin(false);
    not_recursive();
}

pub fn start_recursion() {
    recursion_begin(true);
}

pub fn not_recursive() {
    let _ = ok();
}

extern "C" {
    fn external_function();
}

/// Ensure scanner finds unsafe calls to external functions.
pub fn call_external() {
    unsafe { external_function() };
}
