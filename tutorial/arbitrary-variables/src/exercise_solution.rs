// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! If you want to try yourself the exercise in the Kani tutorial, stop reading now!
//!
//! This file is a solution to that exercise.

use crate::inventory::*;
use std::num::NonZeroU32;
use vector_map::VecMap;

#[cfg(kani)]
mod verification {
    use super::*;

    fn any_inventory(bound: u32) -> Inventory {
        let size: u32 = kani::any();
        kani::assume(size <= bound);

        let mut inner = VecMap::new();

        for _ in 0..size {
            let id: ProductId = kani::any();
            let quantity: NonZeroU32 = kani::any();

            inner.insert(id, quantity);
        }

        Inventory { inner }
    }

    #[kani::proof]
    #[kani::unwind(3)]
    pub fn safe_update_with_any() {
        let mut inventory = any_inventory(0);

        // Create non-deterministic variables for id and quantity.
        let id: ProductId = kani::any();
        let quantity: NonZeroU32 = kani::any();
        assert!(quantity.get() != 0, "NonZeroU32 is internally a u32 but it should never be 0.");

        // Update the inventory and check the result.
        inventory.update(id, quantity);
        assert!(inventory.get(&id).unwrap() == quantity);
    }
}
