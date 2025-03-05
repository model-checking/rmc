// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::str::FromStr;

use crate::args::Timeout;
use crate::args::autoharness_args::{CargoAutoharnessArgs, StandaloneAutoharnessArgs};
use crate::call_cbmc::VerificationStatus;
use crate::call_single_file::to_rustc_arg;
use crate::harness_runner::HarnessResult;
use crate::session::KaniSession;
use crate::{InvocationType, print_kani_version, project, verify_project};
use anyhow::Result;
use comfy_table::Table as PrettyTable;
use kani_metadata::{AutoHarnessSkipReason, KaniMetadata};

const AUTOHARNESS_TIMEOUT: &str = "30s";
const LOOP_UNWIND_DEFAULT: u32 = 20;

pub fn autoharness_cargo(args: CargoAutoharnessArgs) -> Result<()> {
    let mut session = KaniSession::new(args.verify_opts)?;
    session.enable_autoharness();
    session.add_default_bounds();
    session.add_auto_harness_args(
        args.common_autoharness_args.include_function,
        args.common_autoharness_args.exclude_function,
    );
    let project = project::cargo_project(&mut session, false)?;
    let metadata = project.metadata.clone();
    let res = verify_project(project, session);
    print_skipped_fns(metadata);
    res
}

pub fn autoharness_standalone(args: StandaloneAutoharnessArgs) -> Result<()> {
    let mut session = KaniSession::new(args.verify_opts)?;
    session.enable_autoharness();
    session.add_default_bounds();
    session.add_auto_harness_args(
        args.common_autoharness_args.include_function,
        args.common_autoharness_args.exclude_function,
    );

    if !session.args.common_args.quiet {
        print_kani_version(InvocationType::Standalone);
    }

    let project = project::standalone_project(&args.input, args.crate_name, &session)?;
    let metadata = project.metadata.clone();
    let res = verify_project(project, session);
    print_skipped_fns(metadata);
    res
}

/// Print a table of the functions that we skipped and why.
fn print_skipped_fns(metadata: Vec<KaniMetadata>) {
    let mut skipped_fns = PrettyTable::new();
    skipped_fns.set_header(vec!["Skipped Function", "Reason for Skipping"]);

    for md in metadata {
        let skipped_md = md.autoharness_skipped_fns.unwrap();
        skipped_fns.add_rows(skipped_md.into_iter().filter_map(|(func, reason)| match reason {
            AutoHarnessSkipReason::MissingArbitraryImpl(ref args) => {
                Some(vec![func, format!("{}: {}", reason, args.join(", "))])
            }
            AutoHarnessSkipReason::GenericFn
            | AutoHarnessSkipReason::NoBody
            | AutoHarnessSkipReason::UserFilter => Some(vec![func, reason.to_string()]),
            // We don't report Kani implementations to the user to avoid exposing Kani functions we insert during instrumentation.
            // For those we don't insert during instrumentation that are in this category (manual harnesses or Kani trait implementations),
            // it should be obvious that we wouldn't generate harnesses, so reporting those functions as "skipped" is unlikely to be useful.
            AutoHarnessSkipReason::KaniImpl => None,
        }));
    }

    if skipped_fns.is_empty() {
        println!(
            "\nSkipped Functions: None. Kani generated automatic harnesses for all functions in the available crate(s)."
        );
        return;
    }

    println!("\nKani did not generate automatic harnesses for the following functions.");
    println!(
        "If you believe that the provided reason is incorrect and Kani should have generated an automatic harness, please comment on this issue: https://github.com/model-checking/kani/issues/3832"
    );
    println!("{skipped_fns}");
}

impl KaniSession {
    /// Enable autoharness mode.
    pub fn enable_autoharness(&mut self) {
        self.auto_harness = true;
    }

    /// Add the compiler arguments specific to the `autoharness` subcommand.
    pub fn add_auto_harness_args(&mut self, included: Vec<String>, excluded: Vec<String>) {
        for func in included {
            self.pkg_args
                .push(to_rustc_arg(vec![format!("--autoharness-include-function {}", func)]));
        }
        for func in excluded {
            self.pkg_args
                .push(to_rustc_arg(vec![format!("--autoharness-exclude-function {}", func)]));
        }
    }

    /// Add global harness timeout and loop unwinding bounds if not provided.
    /// These prevent automatic harnesses from hanging.
    pub fn add_default_bounds(&mut self) {
        if self.args.harness_timeout.is_none() {
            let timeout = Timeout::from_str(AUTOHARNESS_TIMEOUT).unwrap();
            self.args.harness_timeout = Some(timeout);
        }
        if self.args.default_unwind.is_none() {
            self.args.default_unwind = Some(LOOP_UNWIND_DEFAULT);
        }
    }

    /// Prints the results from running the `autoharness` subcommand.
    pub fn print_autoharness_summary(&self, automatic: Vec<&HarnessResult<'_>>) -> Result<()> {
        let (successes, failures): (Vec<_>, Vec<_>) =
            automatic.into_iter().partition(|r| r.result.status == VerificationStatus::Success);

        let succeeding = successes.len();
        let failing = failures.len();
        let total = succeeding + failing;

        println!("\nAutoharness Summary:");

        let mut verified_fns = PrettyTable::new();
        verified_fns.set_header(vec![
            "Selected Function",
            "Kind of Automatic Harness",
            "Verification Result",
        ]);

        for success in successes {
            verified_fns.add_row(vec![
                success.harness.pretty_name.clone(),
                success.harness.attributes.kind.to_string(),
                success.result.status.to_string(),
            ]);
        }

        for failure in failures {
            verified_fns.add_row(vec![
                failure.harness.pretty_name.clone(),
                failure.harness.attributes.kind.to_string(),
                failure.result.status.to_string(),
            ]);
        }

        println!("{verified_fns}");

        if failing > 0 {
            println!(
                "Note that `kani autoharness` sets default --harness-timeout of {AUTOHARNESS_TIMEOUT} and --default-unwind of {LOOP_UNWIND_DEFAULT}."
            );
            println!(
                "If verification failed because of timing out or too low of an unwinding bound, try passing larger values for these arguments (or, if possible, writing a loop contract)."
            );
        }

        if total > 0 {
            println!(
                "Complete - {succeeding} successfully verified functions, {failing} failures, {total} total."
            );
        } else {
            println!("No functions were eligible for automatic verification.");
        }

        Ok(())
    }
}
