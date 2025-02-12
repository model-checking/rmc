// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::mem;

extern crate kani;
use kani::{kani_exists, kani_forall};

#[kani::proof]
fn main() {
    let original_v = vec![kani::any::<usize>(); 3];
    let v = original_v.clone();
    let v_len = v.len();
    kani::assume(kani::forall!(|i in (0,v_len) | v[i] < 5));

    // Prevent running `v`'s destructor so we are in complete control
    // of the allocation.
    let mut v = mem::ManuallyDrop::new(v);

    // Pull out the various important pieces of information about `v`
    let p = v.as_mut_ptr();
    let len = v.len();
    let cap = v.capacity();

    unsafe {
        // Overwrite memory
        for i in 0..len {
            *p.add(i) += 1;
            if i == 1 {
                *p.add(i) = 0;
            }
        }

        // Put everything back together into a Vec
        let rebuilt = Vec::from_raw_parts(p, len, cap);
        assert!(kani::exists!(| i in (0, len) | rebuilt[i] == 0));
    }
}
