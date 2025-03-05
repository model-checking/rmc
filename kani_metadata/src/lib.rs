// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

extern crate clap;

use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};
use strum_macros::{Display, EnumString};

pub use artifact::ArtifactType;
pub use cbmc_solver::CbmcSolver;
pub use harness::*;
pub use vtable::*;

pub mod artifact;
mod cbmc_solver;
mod harness;
pub mod unstable;
mod vtable;

pub use unstable::{EnabledUnstableFeatures, UnstableFeature};

/// The structure of `.kani-metadata.json` files, which are emitted for each crate
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KaniMetadata {
    /// The crate name from which this metadata was extracted.
    pub crate_name: String,
    /// The proof harnesses (`#[kani::proof]`) found in this crate.
    pub proof_harnesses: Vec<HarnessMetadata>,
    /// The features found in this crate that Kani does not support.
    /// (These general translate to `assert(false)` so we can still attempt verification.)
    pub unsupported_features: Vec<UnsupportedFeature>,
    /// If crates are built in test-mode, then test harnesses will be recorded here.
    pub test_harnesses: Vec<HarnessMetadata>,
    /// The functions with contracts in this crate
    pub contracted_functions: Vec<ContractedFunction>,
    /// Metadata for the `autoharness` subcommand
    pub autoharness_skipped_fns: Option<AutoHarnessSkippedFns>,
}

/// Reasons that Kani does not generate an automatic harness for a function.
#[derive(Debug, Clone, Serialize, Deserialize, Display, EnumString)]
pub enum AutoHarnessSkipReason {
    /// The function is generic.
    #[strum(serialize = "Generic Function")]
    GenericFn,
    /// A Kani-internal function: already a harness, implementation of a Kani associated item or Kani contract instrumentation functions).
    #[strum(serialize = "Kani implementation")]
    KaniImpl,
    /// At least one of the function's arguments does not implement kani::Arbitrary
    /// (The Vec<String> contains the list of argument names that do not implement it)
    #[strum(serialize = "Missing Arbitrary implementation for argument(s)")]
    MissingArbitraryImpl(Vec<String>),
    /// The function does not have a body.
    #[strum(serialize = "The function does not have a body")]
    NoBody,
    /// The function doesn't match the user's provided filters.
    #[strum(serialize = "Did not match provided filters")]
    UserFilter,
}

/// For the autoharness subcommand: map function names to the reason why we did not generate an automatic harness for that function.
/// We use an ordered map so that when we print the data, it is ordered alphabetically by function name.
pub type AutoHarnessSkippedFns = BTreeMap<String, AutoHarnessSkipReason>;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, PartialOrd, Ord)]
pub struct ContractedFunction {
    /// The fully qualified name the user gave to the function (i.e. includes the module path).
    pub function: String,
    /// The (currently full-) path to the file this function was declared within.
    pub file: String,
    /// The pretty names of the proof harnesses (`#[kani::proof_for_contract]`) for this function
    pub harnesses: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsupportedFeature {
    // We could replace this with an enum: https://github.com/model-checking/kani/issues/1765
    /// A string identifying the feature.
    pub feature: String,
    /// A list of locations (file, line) where this unsupported feature can be found.
    pub locations: HashSet<Location>,
}

/// The location in a file
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Location {
    pub filename: String,
    pub start_line: u64,
}

/// We stub artifacts with the path to a KaniMetadata file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilerArtifactStub {
    pub metadata_path: PathBuf,
}
