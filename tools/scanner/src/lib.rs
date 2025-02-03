// SPDX-License-Identifier: Apache-2.0 OR MIT
//
// Modifications Copyright Kani Contributors
// See GitHub history for details.

// This is a modified version of project-stable-mir `test-drive`
// <https://github.com/rust-lang/project-stable-mir/blob/8ec26c61/tools/test-drive/src/main.rs>

//! This library provide different ways of scanning a crate.

#![feature(rustc_private)]

pub mod analysis;
pub mod call_graph;

extern crate rustc_driver;
extern crate rustc_interface;
extern crate rustc_middle;
extern crate rustc_session;
#[macro_use]
extern crate rustc_smir;
extern crate stable_mir;

use crate::analysis::OverallStats;
use rustc_middle::ty::TyCtxt;
use rustc_session::config::OutputType;
use rustc_smir::{run_with_tcx, rustc_internal};
use stable_mir::CompilerError;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use strum::IntoEnumIterator;
use strum_macros::{AsRefStr, EnumIter};

// Use a static variable for simplicity.
static VERBOSE: AtomicBool = AtomicBool::new(false);

pub fn run_all(rustc_args: Vec<String>, verbose: bool) -> ExitCode {
    run_analyses(rustc_args, &Analysis::iter().collect::<Vec<_>>(), verbose)
}

/// Executes a compilation and run the analysis that were requested.
pub fn run_analyses(rustc_args: Vec<String>, analyses: &[Analysis], verbose: bool) -> ExitCode {
    VERBOSE.store(verbose, Ordering::Relaxed);
    let result = run_with_tcx!(rustc_args, |tcx| analyze_crate(tcx, analyses));
    if result.is_ok() || matches!(result, Err(CompilerError::Skipped)) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[derive(AsRefStr, EnumIter, Debug, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum Analysis {
    /// Collect information about generic functions.
    MonoFns,
    /// Collect information about function safety.
    SafeFns,
    /// Collect information about function inputs.
    InputTys,
    /// Collect information about unsafe operations.
    UnsafeOps,
    /// Collect information about loops inside a function.
    FnLoops,
    /// Collect information about recursion via direct calls.
    Recursion,
    /// Collect information about transitive usage of unsafe.
    UnsafeDistance,
}

fn info(msg: String) {
    if VERBOSE.load(Ordering::Relaxed) {
        eprintln!("[INFO] {}", msg);
    }
}

/// This function invoke the required analyses in the given order.
fn analyze_crate(tcx: TyCtxt, analyses: &[Analysis]) -> ControlFlow<()> {
    if stable_mir::local_crate().name == "build_script_build" {
        return ControlFlow::Continue(());
    }
    let object_file = tcx.output_filenames(()).path(OutputType::Object);
    let base_path = object_file.as_path().to_path_buf();
    // Use name for now to make it more friendly. Change to base_path.file_stem() to avoid conflict.
    // let file_stem = base_path.file_stem().unwrap();
    let file_stem = format!("{}_scan", stable_mir::local_crate().name);
    let mut crate_stats = OverallStats::new();
    for analysis in analyses {
        let filename = format!("{}_{}", file_stem, analysis.as_ref());
        let mut out_path = base_path.parent().map_or(PathBuf::default(), Path::to_path_buf);
        out_path.set_file_name(filename);
        match analysis {
            Analysis::MonoFns => {
                crate_stats.generic_fns();
            }
            Analysis::SafeFns => {
                crate_stats.safe_fns(out_path);
            }
            Analysis::InputTys => crate_stats.supported_inputs(out_path),
            Analysis::UnsafeOps => crate_stats.unsafe_operations(out_path),
            Analysis::FnLoops => crate_stats.loops(out_path),
            Analysis::Recursion => crate_stats.recursion(out_path),
            Analysis::UnsafeDistance => crate_stats.unsafe_distance(out_path),
        }
    }
    crate_stats.store_csv(base_path, &file_stem);
    ControlFlow::<()>::Continue(())
}
