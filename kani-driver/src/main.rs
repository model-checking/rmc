// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
#![feature(let_chains)]
use std::ffi::OsString;
use std::process::ExitCode;

use anyhow::Result;
use autoharness::{autoharness_cargo, autoharness_standalone};
use time::{OffsetDateTime, format_description};

use args::{CargoKaniSubcommand, check_is_valid};
use args_toml::join_args;

use crate::args::StandaloneSubcommand;
use crate::concrete_playback::playback::{playback_cargo, playback_standalone};
use crate::list::collect_metadata::{list_cargo, list_standalone};
use crate::project::Project;
use crate::session::KaniSession;
use crate::version::print_kani_version;
use clap::Parser;
use tracing::debug;

mod args;
mod args_toml;
mod assess;
mod autoharness;
mod call_cargo;
mod call_cbmc;
mod call_goto_cc;
mod call_goto_instrument;
mod call_goto_synthesizer;
mod call_single_file;
mod cbmc_output_parser;
mod cbmc_property_renderer;
mod concrete_playback;
mod coverage;
mod harness_runner;
mod list;
mod metadata;
mod project;
mod session;
mod util;
mod version;

/// The main function for the `kani-driver`.
/// The driver can be invoked via `cargo kani` and `kani` commands, which determines what kind of
/// project should be verified.
fn main() -> ExitCode {
    let invocation_type = determine_invocation_type(Vec::from_iter(std::env::args_os()));

    let result = match invocation_type {
        InvocationType::CargoKani(args) => cargokani_main(args),
        InvocationType::Standalone => standalone_main(),
    };

    if let Err(error) = result {
        // We are using the debug format for now to print the all the context.
        // We should consider creating a standard for error reporting.
        debug!(?error, "main_failure");
        util::error(&format!("{error:#}"));
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The main function for the `cargo kani` command.
fn cargokani_main(input_args: Vec<OsString>) -> Result<()> {
    let input_args = join_args(input_args)?;
    let args = args::CargoKaniArgs::parse_from(&input_args);
    check_is_valid(&args);

    if let Some(CargoKaniSubcommand::List(list_args)) = args.command {
        return list_cargo(*list_args, args.verify_opts);
    }

    if let Some(CargoKaniSubcommand::Autoharness(autoharness_args)) = args.command {
        return autoharness_cargo(*autoharness_args);
    }

    let mut session = match args.command {
        Some(CargoKaniSubcommand::Assess(assess_args)) => {
            let sess = session::KaniSession::new(args.verify_opts)?;
            return assess::run_assess(sess, *assess_args);
        }
        Some(CargoKaniSubcommand::Playback(args)) => {
            return playback_cargo(*args);
        }
        Some(CargoKaniSubcommand::Autoharness(_)) | Some(CargoKaniSubcommand::List(_)) => {
            unreachable!()
        }
        None => session::KaniSession::new(args.verify_opts)?,
    };

    if !session.args.common_args.quiet {
        print_kani_version(InvocationType::CargoKani(input_args));
    }

    if session.args.assess {
        return assess::run_assess(session, assess::AssessArgs::default());
    }

    let project = project::cargo_project(&mut session, false)?;
    if session.args.only_codegen { Ok(()) } else { verify_project(project, session) }
}

/// The main function for the `kani` command.
fn standalone_main() -> Result<()> {
    let args = args::StandaloneArgs::parse();
    check_is_valid(&args);

    let (session, project) = match args.command {
        Some(StandaloneSubcommand::Autoharness(args)) => {
            return autoharness_standalone(*args);
        }
        Some(StandaloneSubcommand::Playback(args)) => return playback_standalone(*args),
        Some(StandaloneSubcommand::List(list_args)) => {
            return list_standalone(*list_args, args.verify_opts);
        }
        Some(StandaloneSubcommand::VerifyStd(args)) => {
            let session = KaniSession::new(args.verify_opts)?;
            if !session.args.common_args.quiet {
                print_kani_version(InvocationType::Standalone);
            }

            let project = project::std_project(&args.std_path, &session)?;
            (session, project)
        }
        None => {
            let session = KaniSession::new(args.verify_opts)?;
            if !session.args.common_args.quiet {
                print_kani_version(InvocationType::Standalone);
            }

            let project =
                project::standalone_project(&args.input.unwrap(), args.crate_name, &session)?;
            (session, project)
        }
    };
    if session.args.only_codegen { Ok(()) } else { verify_project(project, session) }
}

/// Run verification on the given project.
fn verify_project(project: Project, session: KaniSession) -> Result<()> {
    debug!(?project, "verify_project");
    let harnesses = session.determine_targets(&project.get_all_harnesses())?;
    debug!(n = harnesses.len(), ?harnesses, "verify_project");

    // Verification
    let runner = harness_runner::HarnessRunner { sess: &session, project: &project };
    let results = runner.check_all_harnesses(&harnesses)?;

    if session.args.coverage {
        // We generate a timestamp to save the coverage data in a folder named
        // `kanicov_<date>` where `<date>` is the current date based on `format`
        // below. The purpose of adding timestamps to the folder name is to make
        // coverage results easily identifiable. Using a timestamp makes
        // coverage results not only distinguishable, but also easy to relate to
        // verification runs. We expect this to be particularly helpful for
        // users in a proof debugging session, who are usually interested in the
        // most recent results.
        let time_now = OffsetDateTime::now_utc();
        let format = format_description::parse("[year]-[month]-[day]_[hour]-[minute]").unwrap();
        let timestamp = time_now.format(&format).unwrap();

        session.save_coverage_metadata(&project, &timestamp)?;
        session.save_coverage_results(&project, &results, &timestamp)?;
    }

    session.print_final_summary(&results)
}

#[derive(Debug, PartialEq, Eq)]
enum InvocationType {
    CargoKani(Vec<OsString>),
    Standalone,
}

/// Peeks at command line arguments to determine if we're being invoked as 'kani' or 'cargo-kani'
fn determine_invocation_type(mut args: Vec<OsString>) -> InvocationType {
    let exe = util::executable_basename(&args.first());

    // Case 1: if 'kani' is our first real argument, then we're being invoked as cargo-kani
    // 'cargo kani ...' will cause cargo to run 'cargo-kani kani ...' preserving argv1
    if Some(&OsString::from("kani")) == args.get(1) {
        // Recreate our command line, but with 'kani' skipped
        args.remove(1);
        InvocationType::CargoKani(args)
    }
    // Case 2: if 'kani' is the name we're invoked as, then we're being invoked standalone
    // Note: we care about argv0 here, NOT std::env::current_exe(), as the later will be resolved
    else if Some("kani".into()) == exe {
        InvocationType::Standalone
    }
    // Case 3: if 'cargo-kani' is the name we're invoked as, then the user is directly invoking
    // 'cargo-kani' instead of 'cargo kani', and we shouldn't alter arguments.
    else if Some("cargo-kani".into()) == exe {
        InvocationType::CargoKani(args)
    }
    // Case 4: default fallback, act like standalone
    else {
        InvocationType::Standalone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_invocation_type() {
        // conversions to/from OsString are rough, simplify the test code below
        fn x(args: Vec<&str>) -> Vec<OsString> {
            args.iter().map(|x| x.into()).collect()
        }

        // Case 1: 'cargo kani'
        assert_eq!(
            determine_invocation_type(x(vec!["bar", "kani", "foo"])),
            InvocationType::CargoKani(x(vec!["bar", "foo"]))
        );
        // Case 3: 'cargo-kani'
        assert_eq!(
            determine_invocation_type(x(vec!["cargo-kani", "foo"])),
            InvocationType::CargoKani(x(vec!["cargo-kani", "foo"]))
        );
        // Case 2: 'kani'
        assert_eq!(determine_invocation_type(x(vec!["kani", "foo"])), InvocationType::Standalone);
        // default
        assert_eq!(determine_invocation_type(x(vec!["foo"])), InvocationType::Standalone);
        // weird case can be handled
        assert_eq!(determine_invocation_type(x(vec![])), InvocationType::Standalone);
    }
}
