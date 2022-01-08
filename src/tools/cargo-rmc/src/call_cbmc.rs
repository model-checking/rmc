// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::context::RmcContext;

impl RmcContext {
    /// Verify a goto binary that's been prepared with goto-instrument
    pub fn run_cbmc(&self, file: &Path) -> Result<PathBuf> {
        let output_filename = crate::util::append_path(file, "cbmc_output");

        {
            let mut temps = self.temporaries.borrow_mut();
            temps.push(output_filename.clone());
        }

        let output_file = std::fs::File::create(&output_filename)?;

        let args: Vec<OsString> = self.cbmc_flags(file)?;

        // TODO get cbmc path from self
        let result = Command::new("cbmc")
            .args(args)
            .stdout(Stdio::from(output_file))
            .status()
            .context("Failed to invoke cbmc")?;

        // regardless of success or failure, first:
        self.format_cbmc_output(&output_filename)?;

        if !result.success() {
            bail!("cbmc exited with status {}", result);
        }

        Ok(output_filename)
    }

    /// used by call_cbmc_viewer, needs refactor TODO
    pub fn call_cbmc(&self, args: Vec<OsString>, output: Stdio) -> Result<()> {
        // TODO get cbmc path from self
        let result = Command::new("cbmc")
            .args(args)
            .stdout(output)
            .status()
            .context("Failed to invoke cbmc")?;

        if !result.success() {
            bail!("cbmc exited with status {}", result);
        }

        Ok(())
    }

    /// "Internal," but also used by call_cbmc_viewer
    pub fn cbmc_flags(&self, file: &Path) -> Result<Vec<OsString>> {
        let args: Vec<OsString> = vec![
            "--bounds-check".into(),
            "--pointer-check".into(),
            "--pointer-primitive-check".into(),
            "--conversion-check".into(),
            "--div-by-zero-check".into(),
            "--float-overflow-check".into(),
            "--nan-check".into(),
            "--pointer-overflow-check".into(),
            "--undefined-shift-check".into(),
            "--unwinding-assertions".into(),
            "--object-bits".into(),
            "16".into(),
            "--json-ui".into(), // todo unconditional, we always redirect output
            // but todo: we're appending --xml-ui for viewer, which works because it seems to override, but that's unclean
            file.to_owned().into_os_string(),
        ];

        Ok(args)
    }
}
