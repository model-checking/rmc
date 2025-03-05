// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use anyhow::{Result, bail};
use std::path::Path;
use tracing::{debug, trace};

use kani_metadata::{
    HarnessMetadata, InternedString, KaniMetadata, TraitDefinedMethod, VtableCtxResults,
};
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufReader, BufWriter};

use crate::session::KaniSession;
use serde::Deserialize;

/// From either a file or a path with multiple files, output the CBMC restrictions file we should use.
pub fn collect_and_link_function_pointer_restrictions(
    path: &Path,
    output_filename: &Path,
) -> Result<()> {
    let md = std::fs::metadata(path)?;

    // Fill with data from all files in that path with the expected suffix
    let mut per_crate_restrictions = Vec::new();

    if md.is_dir() {
        for element in path.read_dir()? {
            let path = element?.path();
            if path.as_os_str().to_str().unwrap().ends_with(".restrictions.json") {
                let restrictions = from_json(&path)?;
                per_crate_restrictions.push(restrictions);
            }
        }
    } else if md.is_file() {
        assert!(path.as_os_str().to_str().unwrap().ends_with(".restrictions.json"));
        let restrictions = from_json(path)?;
        per_crate_restrictions.push(restrictions);
    } else {
        unreachable!("Path must be restrcitions file or directory containing restrictions files")
    }

    link_function_pointer_restrictions(per_crate_restrictions, output_filename)
}

/// Collect all vtable restriction metadata together, and write one combined output in CBMC's format
fn link_function_pointer_restrictions(
    data_per_crate: Vec<VtableCtxResults>,
    output_filename: &Path,
) -> Result<()> {
    // Combine all method possibilities into one global mapping
    let mut combined_possible_methods: HashMap<TraitDefinedMethod, Vec<InternedString>> =
        HashMap::new();
    for crate_data in &data_per_crate {
        for entry in &crate_data.possible_methods {
            combined_possible_methods
                .insert(entry.trait_method.clone(), entry.possibilities.clone());
        }
    }

    // Emit a restriction for every call site
    let mut output = HashMap::new();
    for crate_data in data_per_crate {
        for call_site in crate_data.call_sites {
            // CBMC Now supports referencing callsites by label:
            // https://github.com/diffblue/cbmc/pull/6508
            let cbmc_call_site_name = format!("{}.{}", call_site.function_name, call_site.label);
            let trait_def = call_site.trait_method;

            // Look up all possibilities, defaulting to the empty set
            let possibilities =
                combined_possible_methods.get(&trait_def).unwrap_or(&vec![]).clone();
            output.insert(cbmc_call_site_name, possibilities);
        }
    }

    let f = File::create(output_filename)?;
    let f = BufWriter::new(f);
    serde_json::to_writer(f, &output)?;
    Ok(())
}

/// Deserialize a json file into a given structure
pub fn from_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let obj = serde_json::from_reader(reader)?;
    Ok(obj)
}

/// Consumes a vector of parsed metadata, and produces a combined structure
pub fn merge_kani_metadata(files: Vec<KaniMetadata>) -> KaniMetadata {
    let mut result = KaniMetadata {
        crate_name: "cbmc-linked".to_string(),
        proof_harnesses: vec![],
        unsupported_features: vec![],
        test_harnesses: vec![],
        contracted_functions: vec![],
        autoharness_skipped_fns: None,
    };
    for md in files {
        // Note that we're taking ownership of the original vec, and so we can move the data into the new data structure.
        result.proof_harnesses.extend(md.proof_harnesses);
        // TODO: these should be merged via a map to aggregate them all
        // https://github.com/model-checking/kani/issues/1758
        result.unsupported_features.extend(md.unsupported_features);
        result.test_harnesses.extend(md.test_harnesses);
        result.contracted_functions.extend(md.contracted_functions);
        // We do not handle autoharness metadata here, since this function is not reachable from the autoharness subcommand.
    }
    result
}

impl KaniSession {
    /// Determine which function to use as entry point, based on command-line arguments and kani-metadata.
    pub fn determine_targets<'a>(
        &self,
        all_harnesses: &[&'a HarnessMetadata],
    ) -> Result<Vec<&'a HarnessMetadata>> {
        let harnesses = BTreeSet::from_iter(self.args.harnesses.iter());
        let total_harnesses = harnesses.len();
        let all_targets = &harnesses;

        if harnesses.is_empty() {
            Ok(Vec::from(all_harnesses))
        } else {
            let harnesses_found: Vec<&HarnessMetadata> =
                find_proof_harnesses(&harnesses, all_harnesses, self.args.exact);

            // If even one harness was not found with --exact, return an error to user
            if self.args.exact && harnesses_found.len() < total_harnesses {
                let harness_found_names: BTreeSet<&String> =
                    harnesses_found.iter().map(|&h| &h.pretty_name).collect();

                // Check which harnesses are missing from the difference of targets and harnesses_found
                let harnesses_missing: Vec<&String> =
                    all_targets.difference(&harness_found_names).cloned().collect();
                let joined_string = harnesses_missing
                    .iter()
                    .map(|&s| (*s).clone())
                    .collect::<Vec<String>>()
                    .join("`, `");

                bail!(
                    "Failed to match the following harness(es):\n{joined_string}\nPlease specify the fully-qualified name of a harness.",
                );
            }

            Ok(harnesses_found)
        }
    }
}

/// Sort harnesses such that for two harnesses in the same file, it is guaranteed that later
/// appearing harnesses get processed earlier.
/// This is necessary for the concrete playback feature (with in-place unit test modification)
/// because it guarantees that injected unit tests will not change the location of to-be-processed harnesses.
pub fn sort_harnesses_by_loc<'a>(harnesses: &[&'a HarnessMetadata]) -> Vec<&'a HarnessMetadata> {
    let mut harnesses_clone = harnesses.to_vec();
    harnesses_clone.sort_unstable_by(|harness1, harness2| {
        harness1
            .original_file
            .cmp(&harness2.original_file)
            .then(harness1.original_start_line.cmp(&harness2.original_start_line).reverse())
    });
    harnesses_clone
}

/// Search for a proof harness with a particular name.
/// At the present time, we use `no_mangle` so collisions shouldn't happen,
/// but this function is written to be robust against that changing in the future.
fn find_proof_harnesses<'a>(
    targets: &BTreeSet<&String>,
    all_harnesses: &[&'a HarnessMetadata],
    exact_filter: bool,
) -> Vec<&'a HarnessMetadata> {
    debug!(?targets, "find_proof_harness");
    let mut result = vec![];
    for md in all_harnesses.iter() {
        // --harnesses should not select automatic harnesses
        if md.is_automatically_generated {
            continue;
        }
        if exact_filter {
            // Check for exact match only
            if targets.contains(&md.pretty_name) {
                // if exact match found, stop searching
                result.push(*md);
            } else {
                trace!(skip = md.pretty_name, "find_proof_harnesses");
            }
        } else {
            // Either an exact match, or a substring match. We check the exact first since it's cheaper.
            if targets.contains(&md.pretty_name)
                || targets.contains(&md.get_harness_name_unqualified().to_string())
                || targets.iter().any(|target| md.pretty_name.contains(*target))
            {
                result.push(*md);
            } else {
                trace!(skip = md.pretty_name, "find_proof_harnesses");
            }
        }
    }
    result
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use kani_metadata::{HarnessAttributes, HarnessKind};
    use std::path::PathBuf;

    pub fn mock_proof_harness(
        name: &str,
        unwind_value: Option<u32>,
        krate: Option<&str>,
        model_file: Option<PathBuf>,
    ) -> HarnessMetadata {
        let mut attributes = HarnessAttributes::new(HarnessKind::Proof);
        attributes.unwind_value = unwind_value;
        HarnessMetadata {
            pretty_name: name.into(),
            mangled_name: name.into(),
            crate_name: krate.unwrap_or("<unknown>").into(),
            original_file: "<unknown>".into(),
            original_start_line: 0,
            original_end_line: 0,
            attributes,
            goto_file: model_file,
            contract: Default::default(),
            has_loop_contracts: false,
            is_automatically_generated: false,
        }
    }

    #[test]
    fn check_find_proof_harness_without_exact() {
        let harnesses = vec![
            mock_proof_harness("check_one", None, None, None),
            mock_proof_harness("module::check_two", None, None, None),
            mock_proof_harness("module::not_check_three", None, None, None),
        ];
        let ref_harnesses = harnesses.iter().collect::<Vec<_>>();

        // Check with harness filtering
        assert_eq!(
            find_proof_harnesses(
                &BTreeSet::from([&"check_three".to_string()]),
                &ref_harnesses,
                false,
            )
            .len(),
            1
        );
        assert!(
            find_proof_harnesses(
                &BTreeSet::from([&"check_two".to_string()]),
                &ref_harnesses,
                false,
            )
            .first()
            .unwrap()
            .mangled_name
                == "module::check_two"
        );
        assert!(
            find_proof_harnesses(
                &BTreeSet::from([&"check_one".to_string()]),
                &ref_harnesses,
                false,
            )
            .first()
            .unwrap()
            .mangled_name
                == "check_one"
        );
    }

    #[test]
    fn check_find_proof_harness_with_exact() {
        // Check with exact match

        let harnesses = vec![
            mock_proof_harness("check_one", None, None, None),
            mock_proof_harness("module::check_two", None, None, None),
            mock_proof_harness("module::not_check_three", None, None, None),
        ];
        let ref_harnesses = harnesses.iter().collect::<Vec<_>>();

        assert!(
            find_proof_harnesses(
                &BTreeSet::from([&"check_three".to_string()]),
                &ref_harnesses,
                true,
            )
            .is_empty()
        );
        assert!(
            find_proof_harnesses(&BTreeSet::from([&"check_two".to_string()]), &ref_harnesses, true)
                .is_empty()
        );
        assert_eq!(
            find_proof_harnesses(&BTreeSet::from([&"check_one".to_string()]), &ref_harnesses, true)
                .first()
                .unwrap()
                .mangled_name,
            "check_one"
        );
        assert_eq!(
            find_proof_harnesses(
                &BTreeSet::from([&"module::not_check_three".to_string()]),
                &ref_harnesses,
                true,
            )
            .first()
            .unwrap()
            .mangled_name,
            "module::not_check_three"
        );
    }
}
