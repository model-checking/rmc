// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use strum_macros::{AsRefStr, Display, EnumString, VariantNames};
use tracing_subscriber::filter::Directive;

#[derive(Debug, Default, Display, Clone, Copy, AsRefStr, EnumString, VariantNames, PartialEq, Eq)]
#[strum(serialize_all = "snake_case")]
pub enum BackendOption {
    /// CProver (Goto) backend
    #[cfg(feature = "cprover")]
    #[strum(serialize = "cprover")]
    #[default]
    CProver,

    /// LLBC backend (Aeneas's IR)
    #[cfg(feature = "llbc")]
    Llbc,
}

#[derive(Debug, Default, Clone, Copy, AsRefStr, EnumString, VariantNames, PartialEq, Eq)]
#[strum(serialize_all = "snake_case")]
pub enum ReachabilityType {
    /// Start the cross-crate reachability analysis from all harnesses in the local crate.
    Harnesses,
    /// Don't perform any reachability analysis. This will skip codegen for this crate.
    #[default]
    None,
    /// Start the cross-crate reachability analysis from all public functions in the local crate.
    PubFns,
    /// Start the cross-crate reachability analysis from all *test* (i.e. `#[test]`) harnesses in the local crate.
    Tests,
    /// Start the cross-crate reachability analysis from all functions in the local crate.
    /// Currently, this mode is only used for automatic harness generation.
    AllFns,
}

/// Command line arguments that this instance of the compiler run was called
/// with. Usually stored in and accessible via [`crate::kani_queries::QueryDb`].
#[derive(Debug, Default, Clone, clap::Parser)]
pub struct Arguments {
    /// Option used to disable asserting function contracts.
    #[clap(long)]
    pub no_assert_contracts: bool,
    /// Option name used to enable assertion reachability checks.
    #[clap(long = "assertion-reach-checks")]
    pub check_assertion_reachability: bool,
    /// Option name used to enable coverage checks.
    #[clap(long = "coverage-checks")]
    pub check_coverage: bool,
    /// Option name used to dump function pointer restrictions.
    #[clap(long = "restrict-vtable-fn-ptrs")]
    pub emit_vtable_restrictions: bool,
    /// Option name used to use json pretty-print for output files.
    #[clap(long = "pretty-json-files")]
    pub output_pretty_json: bool,
    /// Option used for suppressing global ASM error.
    #[clap(long)]
    pub ignore_global_asm: bool,
    /// Option name used to select which reachability analysis to perform.
    #[clap(long = "reachability", default_value = "none")]
    pub reachability_analysis: ReachabilityType,
    #[clap(long = "enable-stubbing")]
    pub stubbing_enabled: bool,
    /// Option name used to define unstable features.
    #[clap(short = 'Z', long = "unstable")]
    pub unstable_features: Vec<String>,
    #[clap(long)]
    /// Option used for building standard library.
    ///
    /// Flag that indicates that we are currently building the standard library.
    /// Note that `kani` library will not be available if this is `true`.
    pub build_std: bool,
    #[clap(long)]
    /// Option name used to set log level.
    pub log_level: Option<Directive>,
    #[clap(long)]
    /// Option name used to set the log output to a json file.
    pub json_output: bool,
    #[clap(long, conflicts_with = "json_output")]
    /// Option name used to force logger to use color output. This doesn't work with --json-output.
    pub color_output: bool,
    #[clap(long)]
    /// Pass the kani version to the compiler to ensure cache coherence.
    check_version: Option<String>,
    #[clap(long)]
    pub ub_check: Vec<ExtraChecks>,
    /// Option name used to select which backend to use.
    #[clap(long = "backend", default_value_t = BackendOption::CProver)]
    pub backend: BackendOption,
    /// Print the final LLBC file to stdout.
    #[clap(long)]
    pub print_llbc: bool,
    /// If we are running the autoharness subcommand, the functions to include
    #[arg(
        long = "autoharness-include-function",
        num_args(1),
        conflicts_with = "autoharness_excluded_functions"
    )]
    pub autoharness_included_functions: Vec<String>,
    /// If we are running the autoharness subcommand, the functions to exclude
    #[arg(long = "autoharness-exclude-function", num_args(1))]
    pub autoharness_excluded_functions: Vec<String>,
}

#[derive(Debug, Clone, Copy, AsRefStr, EnumString, VariantNames, PartialEq, Eq)]
#[strum(serialize_all = "snake_case")]
pub enum ExtraChecks {
    /// Check that produced values are valid except for uninitialized values.
    /// See https://github.com/model-checking/kani/issues/920.
    Validity,
    /// Check for using uninitialized memory.
    Uninit,
}
