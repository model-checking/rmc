// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

use crate::args::RmcArgs;
use anyhow::{bail, Context, Result};
use std::cell::RefCell;
use std::path::PathBuf;

/// Contains information about the execution environment and arguments that affect operations
pub struct RmcContext {
    /// The common command-line arguments
    pub args: RmcArgs,

    /// The location we found the 'rmc_rustc' command
    pub rmc_rustc: PathBuf,
    /// The location we found 'rmc_lib.c'
    pub rmc_lib_c: PathBuf,
    /// The location we found 'cbmc_json_parser.py'
    pub cbmc_json_parser_py: PathBuf,

    /// The temporary files we littered that need to be cleaned up at the end of execution
    pub temporaries: RefCell<Vec<PathBuf>>,
}

/// Represents where we detected RMC, with helper methods for using that information to find critical paths
enum InstallType {
    /// We're operating in a a checked out repo that's been built locally
    DevRepo(PathBuf),
    // TODO: Once we have something like an installation method, this should represent where we find the files we installed
    //Installed,
}

impl RmcContext {
    pub fn new(args: RmcArgs) -> Result<Self> {
        let install = InstallType::new()?;

        Ok(RmcContext {
            args,
            rmc_rustc: install.rmc_rustc()?,
            rmc_lib_c: install.rmc_lib_c()?,
            cbmc_json_parser_py: install.cbmc_json_parser_py()?,
            temporaries: RefCell::new(vec![]),
        })
    }

    pub fn cleanup(self) {
        if !self.args.keep_temps {
            let temporaries = self.temporaries.borrow();

            for file in temporaries.iter() {
                // If it fails, we don't care, skip it
                let _result = std::fs::remove_file(file);
            }
        }
    }
}

impl InstallType {
    pub fn new() -> Result<Self> {
        let mut exe = std::env::current_exe()
            .context("cargo-rmc was unable to determine where its executable was located")?;
        // Remove the executable name, so we're in the directory we care about
        exe.pop();

        println!("{:?}", exe);

        // Case 1: We've checked out the development repo and we're built under `target/`
        if exe.ends_with("target/debug") {
            exe.pop();
            exe.pop();

            Ok(InstallType::DevRepo(exe))
        } else {
            bail!(
                "Unable to determine installation location. {} doesn't look typical",
                exe.display()
            )
        }
    }

    pub fn rmc_rustc(&self) -> Result<PathBuf> {
        match self {
            Self::DevRepo(repo) => {
                let mut path = repo.clone();
                path.push("scripts/rmc-rustc");
                if path.as_path().exists() {
                    Ok(path)
                } else {
                    bail!("Unable to find rmc-rustc. Looked for {}", path.display());
                }
            }
        }
    }

    pub fn rmc_lib_c(&self) -> Result<PathBuf> {
        match self {
            Self::DevRepo(repo) => {
                let mut path = repo.clone();
                path.push("library/rmc/rmc_lib.c");
                if path.as_path().exists() {
                    Ok(path)
                } else {
                    bail!("Unable to find rmc_lib.c. Looked for {}", path.display());
                }
            }
        }
    }

    pub fn cbmc_json_parser_py(&self) -> Result<PathBuf> {
        match self {
            Self::DevRepo(repo) => {
                let mut path = repo.clone();
                path.push("scripts/cbmc_json_parser.py");
                if path.as_path().exists() {
                    Ok(path)
                } else {
                    bail!("Unable to find cbmc_json_parser.py. Looked for {}", path.display());
                }
            }
        }
    }
}
