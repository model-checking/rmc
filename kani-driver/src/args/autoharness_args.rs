// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Implements the subcommand handling of the autoharness subcommand

use std::path::PathBuf;

use crate::args::{ValidateArgs, VerificationArgs};
use clap::{Error, Parser, error::ErrorKind};
use kani_metadata::UnstableFeature;

#[derive(Debug, Parser)]
pub struct CommonAutoharnessArgs {
    /// If specified, only autoharness functions that match this filter. This option can be provided
    /// multiple times, which will verify all functions matching any of the filters.
    /// Note that this filter will match against partial names, i.e., providing the name of a module will include all functions from that module.
    /// Also note that if the function specified is unable to be automatically verified, this flag will have no effect.
    #[arg(
        long = "include-function",
        num_args(1),
        value_name = "FUNCTION",
        conflicts_with = "exclude_function"
    )]
    pub include_function: Vec<String>,

    /// If specified, only autoharness functions that do not match this filter. This option can be provided
    /// multiple times, which will verify all functions that do not match any of the filters.
    /// Note that this filter will match against partial names, i.e., providing the name of a module will exclude all functions from that module.
    #[arg(long = "exclude-function", num_args(1), value_name = "FUNCTION")]
    pub exclude_function: Vec<String>,
    // TODO: It would be nice if we could borrow --exact here from VerificationArgs to differentiate between partial/exact matches,
    // like --harnesses does. Sharing arguments with VerificationArgs doesn't work with our current structure, though.
}

/// Automatically verify functions in a crate.
#[derive(Debug, Parser)]
pub struct CargoAutoharnessArgs {
    #[command(flatten)]
    pub common_autoharness_args: CommonAutoharnessArgs,

    #[command(flatten)]
    pub verify_opts: VerificationArgs,
}

/// Automatically verify functions in a file.
#[derive(Debug, Parser)]
pub struct StandaloneAutoharnessArgs {
    /// Rust crate's top file location.
    #[arg(required = true)]
    pub input: PathBuf,

    #[arg(long, hide = true)]
    pub crate_name: Option<String>,

    #[command(flatten)]
    pub common_autoharness_args: CommonAutoharnessArgs,

    #[command(flatten)]
    pub verify_opts: VerificationArgs,
}

impl ValidateArgs for CargoAutoharnessArgs {
    fn validate(&self) -> Result<(), Error> {
        self.verify_opts.validate()?;
        if !self.verify_opts.common_args.unstable_features.contains(UnstableFeature::Autoharness) {
            return Err(Error::raw(
                ErrorKind::MissingRequiredArgument,
                format!(
                    "The `autoharness` subcommand is unstable and requires -Z {}",
                    UnstableFeature::Autoharness
                ),
            ));
        }

        if self
            .verify_opts
            .common_args
            .unstable_features
            .contains(UnstableFeature::ConcretePlayback)
        {
            return Err(Error::raw(
                ErrorKind::ArgumentConflict,
                "The autoharness subcommand does not support concrete playback",
            ));
        }

        Ok(())
    }
}

impl ValidateArgs for StandaloneAutoharnessArgs {
    fn validate(&self) -> Result<(), Error> {
        self.verify_opts.validate()?;
        if !self.verify_opts.common_args.unstable_features.contains(UnstableFeature::Autoharness) {
            return Err(Error::raw(
                ErrorKind::MissingRequiredArgument,
                format!(
                    "The `autoharness` subcommand is unstable and requires -Z {}",
                    UnstableFeature::Autoharness
                ),
            ));
        }
        if !self.input.is_file() {
            return Err(Error::raw(
                ErrorKind::InvalidValue,
                format!(
                    "Invalid argument: Input invalid. `{}` is not a regular file.",
                    self.input.display()
                ),
            ));
        }

        if self
            .verify_opts
            .common_args
            .unstable_features
            .contains(UnstableFeature::ConcretePlayback)
        {
            return Err(Error::raw(
                ErrorKind::ArgumentConflict,
                "The autoharness subcommand does not support concrete playback",
            ));
        }

        Ok(())
    }
}
