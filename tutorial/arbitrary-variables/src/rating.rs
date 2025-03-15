// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

// ANCHOR: rating_enum
#[derive(Copy, Clone)]
#[cfg_attr(kani, derive(kani::Arbitrary))]
pub enum Rating {
    One,
    Two,
    Three,
}
// ANCHOR_END: rating_enum

impl Rating {
    fn as_int(&self) -> u8 {
        match self {
            Rating::One => 1,
            Rating::Two => 2,
            Rating::Three => 3,
        }
    }
}

#[cfg(kani)]
mod verification {
    use super::*;

    // ANCHOR: verify_rating
    #[kani::proof]
    pub fn check_rating() {
        let rating: Rating = kani::any();
        assert!((1..=3).contains(&rating.as_int()));
    }
    // ANCHOR_END: verify_rating
}

/// Just an example on how the same could be achieved via an aux function
#[cfg(kani)]
mod expanded {
    use super::*;

    // ANCHOR: rating_arbitrary
    pub fn any_rating() -> Rating {
        match kani::any() {
            0 => Rating::One,
            1 => Rating::Two,
            _ => Rating::Three,
        }
    }
    // ANCHOR_END: rating_arbitrary
}
