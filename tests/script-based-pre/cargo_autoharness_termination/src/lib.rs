// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// Test that the default bounds for automatic harnesses takes effect
// to terminate harnesses that would otherwise run forever.

// Test that the --default-unwind makes harnesses terminate.
// (Technically, the harness timeout could take effect before the unwind bound,
// but in practice the unwind bound is low enough that it always takes effect first.)
mod unwind_bound {
    fn infinite_loop() {
        loop {}
    }

    fn gcd_recursion(x: u64, y: u64) -> u64 {
        let mut max = x;
        let mut min = y;
        if min > max {
            let val = max;
            max = min;
            min = val;
        }
        let res = max % min;
        if res == 0 { min } else { gcd_recursion(min, res + 1) }
    }
}

// Test that when there is no loop/recursion unwinding, the default harness timeout terminates the harness eventually.
mod timeout {
    fn check_harness_timeout() {
        // construct a problem that requires a long time to solve
        let (a1, b1, c1): (u64, u64, u64) = kani::any();
        let (a2, b2, c2): (u64, u64, u64) = kani::any();
        let p1 = a1.saturating_mul(b1).saturating_mul(c1);
        let p2 = a2.saturating_mul(b2).saturating_mul(c2);
        assert!(a1 != a2 || b1 != b2 || c1 != c2 || p1 == p2)
    }
}
