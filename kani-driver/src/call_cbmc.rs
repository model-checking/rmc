// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use anyhow::{Result, bail};
use kani_metadata::{CbmcSolver, HarnessMetadata};
use regex::Regex;
use rustc_demangle::demangle;
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::ffi::OsString;
use std::fmt::Write;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::process::Command as TokioCommand;

use crate::args::common::Verbosity;
use crate::args::{OutputFormat, VerificationArgs};
use crate::cbmc_output_parser::{
    CheckStatus, Property, VerificationOutput, extract_results, process_cbmc_output,
};
use crate::cbmc_property_renderer::{format_coverage, format_result, kani_cbmc_output_filter};
use crate::coverage::cov_results::{CoverageCheck, CoverageResults};
use crate::coverage::cov_results::{CoverageRegion, CoverageTerm};
use crate::session::KaniSession;
use crate::util::render_command;

/// We will use Cadical by default since it performed better than MiniSAT in our analysis.
/// Note: Kissat was marginally better, but it is an external solver which could be more unstable.
static DEFAULT_SOLVER: CbmcSolver = CbmcSolver::Cadical;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationStatus {
    Success,
    Failure,
}

/// Represents failed properties in three different categories.
/// This simplifies the process to determine and format verification results.
#[derive(Clone, Copy, Debug)]
pub enum FailedProperties {
    // No failures
    None,
    // One or more panic-related failures
    PanicsOnly,
    // One or more failures that aren't panic-related
    Other,
}

/// The possible CBMC exit statuses
#[derive(Clone, Copy, Debug)]
pub enum ExitStatus {
    Timeout,
    OutOfMemory,
    /// the integer is the process exit status
    Other(i32),
}

/// Our (kani-driver) notions of CBMC results.
#[derive(Debug)]
pub struct VerificationResult {
    /// Whether verification should be considered to have succeeded, or have failed.
    pub status: VerificationStatus,
    /// The compact representation for failed properties
    pub failed_properties: FailedProperties,
    /// The `Result` properties in detail or the exit_status of CBMC.
    /// Note: CBMC process exit status is only potentially useful if `status` is `Failure`.
    /// Kani will see CBMC report "failure" that's actually success (interpreting "failed"
    /// checks like coverage as expected and desirable.)
    pub results: Result<Vec<Property>, ExitStatus>,
    /// The runtime duration of this CBMC invocation.
    pub runtime: Duration,
    /// Whether concrete playback generated a test
    pub generated_concrete_test: bool,
    /// The coverage results
    pub coverage_results: Option<CoverageResults>,
}

impl KaniSession {
    /// Verify a goto binary that's been prepared with goto-instrument
    pub fn run_cbmc(&self, file: &Path, harness: &HarnessMetadata) -> Result<VerificationResult> {
        let args: Vec<OsString> = self.cbmc_flags(file, harness)?;

        // TODO get cbmc path from self
        let mut cmd = TokioCommand::new("cbmc");
        cmd.args(args);

        let verification_results = if self.args.output_format == crate::args::OutputFormat::Old {
            if self.run_terminal_timeout(cmd).is_err() {
                VerificationResult::mock_failure()
            } else {
                VerificationResult::mock_success()
            }
        } else {
            // Add extra argument to receive the output in JSON format.
            // Done here because now removed `--visualize` used the XML format instead.
            // TODO: move this now that we don't use --visualize
            cmd.arg("--json-ui");

            self.runtime.block_on(self.run_cbmc_piped(cmd, harness))?
        };

        Ok(verification_results)
    }

    async fn run_cbmc_piped(
        &self,
        mut cmd: TokioCommand,
        harness: &HarnessMetadata,
    ) -> Result<VerificationResult> {
        if self.args.common_args.verbose() {
            println!("[Kani] Running: `{}`", render_command(cmd.as_std()).to_string_lossy());
        }
        // Spawn the CBMC process and process its output below
        let mut cbmc_process = cmd
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| anyhow::Error::msg("Failed to run cbmc"))?;

        let start_time = Instant::now();

        let res = if let Some(timeout) = self.args.harness_timeout {
            tokio::time::timeout(
                timeout.into(),
                process_cbmc_output(&mut cbmc_process, |i| {
                    kani_cbmc_output_filter(
                        i,
                        self.args.extra_pointer_checks,
                        self.args.common_args.quiet,
                        &self.args.output_format,
                    )
                }),
            )
            .await
        } else {
            Ok(process_cbmc_output(&mut cbmc_process, |i| {
                kani_cbmc_output_filter(
                    i,
                    self.args.extra_pointer_checks,
                    self.args.common_args.quiet,
                    &self.args.output_format,
                )
            })
            .await)
        };

        let verification_results = if res.is_err() {
            // An error occurs if the timeout was reached

            // Kill the process
            cbmc_process.kill().await?;

            VerificationResult {
                status: VerificationStatus::Failure,
                failed_properties: FailedProperties::None,
                results: Err(ExitStatus::Timeout),
                runtime: start_time.elapsed(),
                generated_concrete_test: false,
                coverage_results: None,
            }
        } else {
            // The timeout wasn't reached
            let output = res.unwrap()?;
            VerificationResult::from(output, harness.attributes.should_panic, start_time)
        };

        Ok(verification_results)
    }

    /// "Internal," but also used by call_cbmc_viewer
    pub fn cbmc_flags(
        &self,
        file: &Path,
        harness_metadata: &HarnessMetadata,
    ) -> Result<Vec<OsString>> {
        let mut args = self.cbmc_check_flags();

        if let Some(object_bits) = self.args.cbmc_object_bits() {
            args.push("--object-bits".into());
            args.push(object_bits.to_string().into());
        }

        if let Some(unwind_value) = resolve_unwind_value(&self.args, harness_metadata) {
            args.push("--unwind".into());
            args.push(unwind_value.to_string().into());
        }

        self.handle_solver_args(&harness_metadata.attributes.solver, &mut args)?;

        if self.args.run_sanity_checks {
            args.push("--validate-goto-model".into());
            args.push("--validate-ssa-equation".into());
        }

        if self.args.concrete_playback.is_none() && !self.args.no_slice_formula {
            args.push("--slice-formula".into());
        }

        if self.args.concrete_playback.is_some() {
            args.push("--trace".into());
        }

        args.extend(self.args.cbmc_args.iter().cloned());

        args.push(file.to_owned().into_os_string());

        // Make CBMC verbose by default to tell users about unwinding progress. This should be
        // reviewed as CBMC's verbosity defaults evolve.
        args.push("--verbosity".into());
        args.push("9".into());

        Ok(args)
    }

    /// Just the flags to CBMC that enable property checking of any sort.
    pub fn cbmc_check_flags(&self) -> Vec<OsString> {
        let mut args = Vec::new();

        // We assume that malloc cannot fail, see https://github.com/model-checking/kani/issues/891
        args.push("--no-malloc-may-fail".into());

        // With PR #2630 we generate the appropriate checks directly rather than relying on CBMC's
        // checks (which are for C semantics).
        args.push("--no-undefined-shift-check".into());
        // With PR #647 we use Rust's `-C overflow-checks=on` instead of:
        // --unsigned-overflow-check
        // --signed-overflow-check
        // So these options are deliberately skipped to avoid erroneously re-checking operations.
        args.push("--no-signed-overflow-check".into());

        if !self.args.checks.memory_safety_on() {
            args.push("--no-bounds-check".into());
            args.push("--no-pointer-check".into());
        }
        if self.args.checks.overflow_on() {
            args.push("--nan-check".into());

            // TODO: Implement conversion checks as an optional check.
            // They are a well defined operation in rust, but they may yield unexpected results to
            // many users. https://github.com/model-checking/kani/issues/840
            // We might want to create a transformation pass instead of enabling CBMC since Kani
            // compiler sometimes rely on the bitwise conversion of signed <-> unsigned.
            // args.push("--conversion-check".into());
        } else {
            args.push("--no-div-by-zero-check".into());
        }

        if !self.args.checks.unwinding_on() {
            args.push("--no-unwinding-assertions".into());
        } else {
            args.push("--no-self-loops-to-assumptions".into());
        }

        if self.args.extra_pointer_checks {
            // This was adding a lot of false positives with std dangling pointer. We should
            // still catch any invalid dereference with --pointer-check. Thus, only enable them
            // if the user explicitly request them.
            args.push("--pointer-overflow-check".into());
        } else {
            args.push("--no-pointer-primitive-check".into());
        }

        args
    }

    pub fn handle_solver_args(
        &self,
        harness_solver: &Option<CbmcSolver>,
        args: &mut Vec<OsString>,
    ) -> Result<()> {
        let solver = if let Some(solver) = &self.args.solver {
            // `--solver` option takes precedence over attributes
            solver
        } else if let Some(solver) = harness_solver {
            solver
        } else {
            &DEFAULT_SOLVER
        };

        match solver {
            CbmcSolver::Cadical => {
                args.push("--sat-solver".into());
                args.push("cadical".into());
            }
            CbmcSolver::Kissat => {
                args.push("--external-sat-solver".into());
                args.push("kissat".into());
            }
            CbmcSolver::Minisat => {
                // Minisat is currently CBMC's default solver, so no need to
                // pass any arguments
            }
            CbmcSolver::Binary(solver_binary) => {
                // Check if the specified binary exists in path
                if which::which(solver_binary).is_err() {
                    bail!("the specified solver \"{solver_binary}\" was not found in path")
                }
                args.push("--external-sat-solver".into());
                args.push(solver_binary.into());
            }
        }
        Ok(())
    }
}

impl VerificationResult {
    /// Computes a `VerificationResult` (kani-driver's notion of the result of a CBMC call) from a
    /// `VerificationOutput` (cbmc_output_parser's idea of CBMC results).
    ///
    /// NOTE: We actually ignore the CBMC exit status, in favor of two checks:
    ///   1. Examining the actual results of CBMC properties.
    ///      (CBMC will regularly report "failure" but that's just our cover checks.)
    ///   2. Positively checking for the presence of results.
    ///      (Do not mistake lack of results for success: report it as failure.)
    fn from(
        output: VerificationOutput,
        should_panic: bool,
        start_time: Instant,
    ) -> VerificationResult {
        let runtime = start_time.elapsed();
        let (_, results) = extract_results(output.processed_items);

        if let Some(results) = results {
            let (status, failed_properties) =
                verification_outcome_from_properties(&results, should_panic);
            let coverage_results = coverage_results_from_properties(&results);
            VerificationResult {
                status,
                failed_properties,
                results: Ok(results),
                runtime,
                generated_concrete_test: false,
                coverage_results,
            }
        } else {
            // We never got results from CBMC - something went wrong (e.g. crash) so it's failure
            let exit_status = if output.process_status == 137 {
                ExitStatus::OutOfMemory
            } else {
                ExitStatus::Other(output.process_status)
            };
            VerificationResult {
                status: VerificationStatus::Failure,
                failed_properties: FailedProperties::Other,
                results: Err(exit_status),
                runtime,
                generated_concrete_test: false,
                coverage_results: None,
            }
        }
    }

    pub fn mock_success() -> VerificationResult {
        VerificationResult {
            status: VerificationStatus::Success,
            failed_properties: FailedProperties::None,
            results: Ok(vec![]),
            runtime: Duration::from_secs(0),
            generated_concrete_test: false,
            coverage_results: None,
        }
    }

    fn mock_failure() -> VerificationResult {
        VerificationResult {
            status: VerificationStatus::Failure,
            failed_properties: FailedProperties::Other,
            // on failure, exit codes in theory might be used,
            // but `mock_failure` should never be used in a context where they will,
            // so again use something weird:
            results: Err(ExitStatus::Other(42)),
            runtime: Duration::from_secs(0),
            generated_concrete_test: false,
            coverage_results: None,
        }
    }

    pub fn render(&self, output_format: &OutputFormat, should_panic: bool) -> String {
        match &self.results {
            Ok(results) => {
                let status = self.status;
                let failed_properties = self.failed_properties;
                let show_checks = matches!(output_format, OutputFormat::Regular);

                let mut result = if let Some(cov_results) = &self.coverage_results {
                    format_coverage(
                        results,
                        cov_results,
                        status,
                        should_panic,
                        failed_properties,
                        show_checks,
                    )
                } else {
                    format_result(results, status, should_panic, failed_properties, show_checks)
                };
                writeln!(result, "Verification Time: {}s", self.runtime.as_secs_f32()).unwrap();
                result
            }
            Err(exit_status) => {
                let verification_result = console::style("FAILED").red();
                let (header, explanation) = match exit_status {
                    ExitStatus::OutOfMemory => (
                        String::from("CBMC failed"),
                        "CBMC appears to have run out of memory. You may want to rerun your proof in \
                    an environment with additional memory or use stubbing to reduce the size of the \
                    code the verifier reasons about.\n",
                    ),
                    ExitStatus::Timeout => (
                        String::from("CBMC failed"),
                        "CBMC timed out. You may want to rerun your proof with a larger timeout \
                    or use stubbing to reduce the size of the code the verifier reasons about.\n",
                    ),
                    ExitStatus::Other(exit_status) => {
                        (format!("CBMC failed with status {exit_status}"), "")
                    }
                };
                format!(
                    "\n{header}\n\
                    VERIFICATION:- {verification_result}\n\
                    {explanation}",
                )
            }
        }
    }

    /// Find the failed properties from this verification run
    pub fn failed_properties(&self) -> Vec<&Property> {
        if let Ok(properties) = &self.results {
            properties.iter().filter(|prop| prop.status == CheckStatus::Failure).collect()
        } else {
            debug_assert!(false, "expected error to be handled before invoking this function");
            vec![]
        }
    }
}

/// We decide if verification succeeded based on properties, not (typically) on exit code
fn verification_outcome_from_properties(
    properties: &[Property],
    should_panic: bool,
) -> (VerificationStatus, FailedProperties) {
    let failed_properties = determine_failed_properties(properties);
    let status = if should_panic {
        match failed_properties {
            FailedProperties::None | FailedProperties::Other => VerificationStatus::Failure,
            FailedProperties::PanicsOnly => VerificationStatus::Success,
        }
    } else {
        match failed_properties {
            FailedProperties::None => VerificationStatus::Success,
            FailedProperties::PanicsOnly | FailedProperties::Other => VerificationStatus::Failure,
        }
    };
    (status, failed_properties)
}

/// Determines the `FailedProperties` variant that corresponds to an array of properties
fn determine_failed_properties(properties: &[Property]) -> FailedProperties {
    let failed_properties: Vec<&Property> =
        properties.iter().filter(|prop| prop.status == CheckStatus::Failure).collect();
    // Return `FAILURE` if there isn't at least one failed property
    if failed_properties.is_empty() {
        FailedProperties::None
    } else {
        // Check if all failed properties correspond to the `assertion` class.
        // Note: Panics caused by `panic!` and `assert!` fall into this class.
        let all_failed_checks_are_panics =
            failed_properties.iter().all(|prop| prop.property_class() == "assertion");
        if all_failed_checks_are_panics {
            FailedProperties::PanicsOnly
        } else {
            FailedProperties::Other
        }
    }
}

fn coverage_results_from_properties(properties: &[Property]) -> Option<CoverageResults> {
    let cov_properties: Vec<&Property> =
        properties.iter().filter(|p| p.is_code_coverage_property()).collect();

    if cov_properties.is_empty() {
        return None;
    }

    // Postprocessing the coverage results involves matching on the descriptions
    // of code coverage properties with the `counter_re` regex. These are two
    // real examples of such descriptions:
    //
    // ```
    // CounterIncrement(0) $test_cov$ - src/main.rs:5:1 - 6:15
    // ExpressionUsed(0) $test_cov$ - src/main.rs:6:19 - 6:28
    // ```
    //
    // The span is further processed to extract the code region attributes.
    // Ideally, we should have coverage mappings (i.e., the relation between
    // counters and code regions) available in the coverage metadata:
    // <https://github.com/model-checking/kani/issues/3445>. If that were the
    // case, we would not need the spans in these descriptions.
    let counter_re = {
        static COUNTER_RE: OnceLock<Regex> = OnceLock::new();
        COUNTER_RE.get_or_init(|| {
            Regex::new(
                r#"^(?<kind>VirtualCounter\(bcb)(?<counter_num>[0-9]+)\) \$(?<func_name>[^\$]+)\$ - (?<span>.+)"#,
            )
            .unwrap()
        })
    };

    let mut coverage_results: BTreeMap<String, Vec<CoverageCheck>> = BTreeMap::default();

    for prop in cov_properties {
        let mut prop_processed = false;
        if let Some(captures) = counter_re.captures(&prop.description) {
            let counter_num = &captures["counter_num"];
            let function = demangle(&captures["func_name"]).to_string();
            let status = prop.status;
            let span = captures["span"].to_string();

            let counter_id = counter_num.parse().unwrap();
            let term = CoverageTerm::Counter(counter_id);
            let region = CoverageRegion::from_str(span);

            let cov_check = CoverageCheck::new(function, term, region, status);
            let file = cov_check.region.file.clone();

            if let Entry::Vacant(e) = coverage_results.entry(file.clone()) {
                e.insert(vec![cov_check]);
            } else {
                coverage_results.entry(file).and_modify(|checks| checks.push(cov_check));
            }
            prop_processed = true;
        }

        assert!(prop_processed, "error: coverage property not processed\n{prop:?}");
    }

    Some(CoverageResults::new(coverage_results))
}
/// Solve Unwind Value from conflicting inputs of unwind values. (--default-unwind, annotation-unwind, --unwind)
pub fn resolve_unwind_value(
    args: &VerificationArgs,
    harness_metadata: &HarnessMetadata,
) -> Option<u32> {
    // Check for which flag is being passed and prioritize extracting unwind from the
    // respective flag/annotation.
    // If no unwind value is specified, default to 1 to prevent memory consumption issues
    args.unwind.or(harness_metadata.attributes.unwind_value).or(args.default_unwind).or(Some(1))
}

#[cfg(test)]
mod tests {
    use crate::args;
    use crate::metadata::tests::mock_proof_harness;
    use clap::Parser;

    use super::*;

    #[test]
    fn check_resolve_unwind_value() {
        // Command line unwind value for specific harnesses take precedence over default annotation value
        let args_empty = ["kani", "x.rs"];
        let args_only_default = ["kani", "x.rs", "--default-unwind", "2"];
        let args_only_harness = ["kani", "x.rs", "--unwind", "1", "--harness", "check_one"];
        let args_both =
            ["kani", "x.rs", "--default-unwind", "2", "--unwind", "1", "--harness", "check_one"];

        let harness_none = mock_proof_harness("check_one", None, None, None);
        let harness_some = mock_proof_harness("check_one", Some(3), None, None);

        fn resolve(args: &[&str], harness: &HarnessMetadata) -> Option<u32> {
            resolve_unwind_value(
                &args::StandaloneArgs::try_parse_from(args).unwrap().verify_opts,
                harness,
            )
        }

        // test against no unwind annotation
        assert_eq!(resolve(&args_empty, &harness_none), Some(1));
        assert_eq!(resolve(&args_only_default, &harness_none), Some(2));
        assert_eq!(resolve(&args_only_harness, &harness_none), Some(1));
        assert_eq!(resolve(&args_both, &harness_none), Some(1));

        // test against unwind annotation
        assert_eq!(resolve(&args_empty, &harness_some), Some(3));
        assert_eq!(resolve(&args_only_default, &harness_some), Some(3));
        assert_eq!(resolve(&args_only_harness, &harness_some), Some(1));
        assert_eq!(resolve(&args_both, &harness_some), Some(1));
    }
}
