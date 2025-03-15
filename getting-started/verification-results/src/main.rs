// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

#[kani::proof]
// ANCHOR: success_example
fn success_example() {
    let mut sum = 0;
    for i in 1..4 {
        sum += i;
    }
    assert_eq!(sum, 6);
}
// ANCHOR_END: success_example

#[kani::proof]
// ANCHOR: failure_example
fn failure_example() {
    let arr = [1, 2, 3];
    assert_ne!(arr.len(), 3);
}
// ANCHOR_END: failure_example

#[kani::proof]
// ANCHOR: unreachable_example
fn unreachable_example() {
    let x = 5;
    let y = x + 2;
    if x > y {
        assert!(x < 8);
    }
}
// ANCHOR_END: unreachable_example

#[kani::proof]
// ANCHOR: undetermined_example
fn undetermined_example() {
    let mut x = 0;
    unsupp(&mut x);
    assert!(x == 0);
}

#[feature(asm)]
fn unsupp(x: &mut u8) {
    unsafe {
        std::arch::asm!("nop");
    }
}

// ANCHOR_END: undetermined_example

#[kani::proof]
// ANCHOR: cover_satisfied_example
#[kani::unwind(256)]
fn cover_satisfied_example() {
    let mut x: u8 = kani::any();
    let mut y: u8 = kani::any();
    y /= 2;
    let mut i = 0;
    while x != 0 && y != 0 {
        kani::cover!(i > 2 && x == 24 && y == 72);
        if x >= y { x -= y; }
        else { y -= x; }
        i += 1;
    }
}
// ANCHOR_END: cover_satisfied_example

#[kani::proof]
// ANCHOR: cover_unsatisfiable_example
#[kani::unwind(6)]
fn cover_unsatisfiable_example() {
    let bytes: [u8; 5] = kani::any();
    let s = std::str::from_utf8(&bytes);
    if let Ok(s) = s {
        kani::cover!(s.chars().count() <= 1);
    }
}
// ANCHOR_END: cover_unsatisfiable_example

#[kani::proof]
// ANCHOR: cover_unreachable_example
#[kani::unwind(6)]
fn cover_unreachable_example() {
    let r1: std::ops::Range<i32> = kani::any()..kani::any();
    let r2: std::ops::Range<i32> = kani::any()..kani::any();
    kani::assume(!r1.is_empty());
    kani::assume(!r2.is_empty());
    if r2.start > r1.end {
        if r2.end < r1.end {
            kani::cover!(r2.contains(&0));
        }
    }
}
// ANCHOR_END: cover_unreachable_example

fn main() {}
