// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// ANCHOR: inventory_lib
use std::num::NonZeroU32;
use vector_map::VecMap;

pub type ProductId = u32;

pub struct Inventory {
    /// Every product in inventory must have a non-zero quantity
    pub inner: VecMap<ProductId, NonZeroU32>,
}

impl Inventory {
    pub fn update(&mut self, id: ProductId, new_quantity: NonZeroU32) {
        self.inner.insert(id, new_quantity);
    }

    pub fn get(&self, id: &ProductId) -> Option<NonZeroU32> {
        self.inner.get(id).cloned()
    }
}
// ANCHOR_END: inventory_lib

#[cfg(kani)]
mod verification {
    use super::*;

    // ANCHOR: safe_update
    #[kani::proof]
    #[kani::unwind(3)]
    pub fn safe_update() {
        // Empty to start
        let mut inventory = Inventory { inner: VecMap::new() };

        // Create non-deterministic variables for id and quantity.
        let id: ProductId = kani::any();
        let quantity: NonZeroU32 = kani::any();
        assert!(quantity.get() != 0, "NonZeroU32 is internally a u32 but it should never be 0.");

        // Update the inventory and check the result.
        inventory.update(id, quantity);
        assert!(inventory.get(&id).unwrap() == quantity);
    }
    // ANCHOR_END: safe_update
}
