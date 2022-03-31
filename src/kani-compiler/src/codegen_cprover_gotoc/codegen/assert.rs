// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This file contains the code that acts as a wrapper to create the new assert and related statements
use crate::codegen_cprover_gotoc::GotocCtx;
use cbmc::goto_program::{Expr, Location, Stmt};

/// The Property Class enum stores all viable options for classifying asserts, cover assume and other related statements
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum PropertyClass {
    ExpectFail,
    Unimplemented,
    ExactDiv,
    SanityCheck,
    UnsupportedStructs,
    Unreachable,
    Cover,
    PointerOffset,
    AssertFalse,
    Assume,
    DefaultAssertion,
    CustomProperty(String),
}

#[allow(dead_code)]
impl PropertyClass {
    pub fn as_str(&self) -> &str {
        match self {
            PropertyClass::ExpectFail => "expect_fail",
            PropertyClass::Unimplemented => "unimplemented",
            PropertyClass::AssertFalse => "assert_false",
            PropertyClass::Unreachable => "unreachable",
            PropertyClass::Assume => "assume",
            PropertyClass::ExactDiv => "exact_div",
            PropertyClass::Cover => "coverage_check",
            PropertyClass::SanityCheck => "sanity_check",
            PropertyClass::UnsupportedStructs => "unsupported_struct",
            PropertyClass::PointerOffset => "pointer_offset",
            PropertyClass::DefaultAssertion => "assertion",
            PropertyClass::CustomProperty(property_string) => property_string.as_str(),
        }
    }

    pub fn from_str(input: &str) -> PropertyClass {
        match input {
            "expect_fail" => PropertyClass::ExpectFail,
            "unimplemented" => PropertyClass::Unimplemented,
            "assert_false" => PropertyClass::AssertFalse,
            "assume" => PropertyClass::Assume,
            "unreachable" => PropertyClass::Unreachable,
            "exact_div" => PropertyClass::ExactDiv,
            "unsupported_struct" => PropertyClass::UnsupportedStructs,
            "assertion" => PropertyClass::DefaultAssertion,
            "coverage_check" => PropertyClass::Cover,
            "sanity_check" => PropertyClass::SanityCheck,
            "pointer_offset" => PropertyClass::PointerOffset,
            _ => PropertyClass::CustomProperty(input.to_owned()),
        }
    }
}

impl<'tcx> GotocCtx<'tcx> {
    pub fn codegen_assert(
        &self,
        cond: Expr,
        property_class: PropertyClass,
        message: &str,
        loc: Location,
    ) -> Stmt {
        assert!(cond.typ().is_bool());

        let property_name = property_class.as_str();

        // Create a Property Location Variant from any given Location type
        let property_location =
            Location::create_location_with_property(message, property_name, loc);

        Stmt::assert_statement(cond, property_name, message, property_location)
    }
}
