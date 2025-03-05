// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module is responsible for extracting grouping harnesses that can be processed together
//! by codegen.
//!
//! Today, only stub / contracts can affect the harness codegen. Thus, we group the harnesses
//! according to their stub configuration.

use crate::args::{Arguments, ReachabilityType};
use crate::kani_middle::attributes::{KaniAttributes, is_proof_harness};
use crate::kani_middle::kani_functions::{KaniIntrinsic, KaniModel};
use crate::kani_middle::metadata::{
    gen_automatic_proof_metadata, gen_contracts_metadata, gen_proof_metadata,
};
use crate::kani_middle::reachability::filter_crate_items;
use crate::kani_middle::resolve::expect_resolve_fn;
use crate::kani_middle::stubbing::{check_compatibility, harness_stub_map};
use crate::kani_queries::QueryDb;
use kani_metadata::{
    ArtifactType, AssignsContract, AutoHarnessSkipReason, AutoHarnessSkippedFns, HarnessKind,
    HarnessMetadata, KaniMetadata,
};
use rustc_hir::def_id::DefId;
use rustc_middle::ty::TyCtxt;
use rustc_session::config::OutputType;
use rustc_smir::rustc_internal;
use stable_mir::mir::{TerminatorKind, mono::Instance};
use stable_mir::ty::{FnDef, GenericArgKind, GenericArgs, IndexedVal, RigidTy, TyKind};
use stable_mir::{CrateDef, CrateItem};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use tracing::debug;

/// An identifier for the harness function.
type Harness = Instance;

/// A set of stubs.
pub type Stubs = HashMap<FnDef, FnDef>;

/// Store some relevant information about the crate compilation.
#[derive(Clone, Debug)]
struct CrateInfo {
    /// The name of the crate being compiled.
    pub name: String,
}

/// We group the harnesses that have the same stubs.
pub struct CodegenUnits {
    autoharness_skipped_fns: Option<AutoHarnessSkippedFns>,
    crate_info: CrateInfo,
    harness_info: HashMap<Harness, HarnessMetadata>,
    units: Vec<CodegenUnit>,
}

#[derive(Clone, Default, Debug)]
pub struct CodegenUnit {
    pub harnesses: Vec<Harness>,
    pub stubs: Stubs,
}

impl CodegenUnits {
    pub fn new(queries: &QueryDb, tcx: TyCtxt) -> Self {
        let crate_info = CrateInfo { name: stable_mir::local_crate().name.as_str().into() };
        let base_filepath = tcx.output_filenames(()).path(OutputType::Object);
        let base_filename = base_filepath.as_path();
        let args = queries.args();
        match args.reachability_analysis {
            ReachabilityType::Harnesses => {
                let all_harnesses = get_all_manual_harnesses(tcx, base_filename);
                // Even if no_stubs is empty we still need to store rustc metadata.
                let units = group_by_stubs(tcx, &all_harnesses);
                validate_units(tcx, &units);
                debug!(?units, "CodegenUnits::new");
                CodegenUnits {
                    units,
                    harness_info: all_harnesses,
                    crate_info,
                    autoharness_skipped_fns: None,
                }
            }
            ReachabilityType::AllFns => {
                let mut all_harnesses = get_all_manual_harnesses(tcx, base_filename);
                let mut units = group_by_stubs(tcx, &all_harnesses);
                validate_units(tcx, &units);

                let kani_fns = queries.kani_functions();
                let kani_harness_intrinsic =
                    kani_fns.get(&KaniIntrinsic::AutomaticHarness.into()).unwrap();

                let (chosen, skipped) = automatic_harness_partition(
                    tcx,
                    args,
                    *kani_fns.get(&KaniModel::Any.into()).unwrap(),
                );

                let automatic_harnesses = get_all_automatic_harnesses(
                    tcx,
                    chosen,
                    *kani_harness_intrinsic,
                    base_filename,
                );
                // We generate one contract harness per function under contract, so each harness is in its own unit,
                // and these harnesses have no stubs.
                units.extend(
                    automatic_harnesses
                        .keys()
                        .map(|harness| CodegenUnit {
                            harnesses: vec![*harness],
                            stubs: HashMap::default(),
                        })
                        .collect::<Vec<_>>(),
                );
                all_harnesses.extend(automatic_harnesses.clone());

                // No need to validate the units again because validation only checks stubs, and we haven't added any stubs.
                debug!(?units, "CodegenUnits::new");
                CodegenUnits {
                    units,
                    harness_info: all_harnesses,
                    crate_info,
                    autoharness_skipped_fns: Some(skipped),
                }
            }
            _ => {
                // Leave other reachability type handling as is for now.
                CodegenUnits {
                    units: vec![],
                    harness_info: HashMap::default(),
                    crate_info,
                    autoharness_skipped_fns: None,
                }
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &CodegenUnit> {
        self.units.iter()
    }

    /// We store which instance of modifies was generated.
    pub fn store_modifies(&mut self, harness_modifies: &[(Harness, AssignsContract)]) {
        for (harness, modifies) in harness_modifies {
            self.harness_info.get_mut(harness).unwrap().contract = Some(modifies.clone());
        }
    }

    /// We flag that the harness contains usage of loop contracts.
    pub fn store_loop_contracts(&mut self, harnesses: &[Harness]) {
        for harness in harnesses {
            let metadata = self.harness_info.get_mut(harness).unwrap();
            metadata.has_loop_contracts = true;
        }
    }

    /// Write compilation metadata into a file.
    pub fn write_metadata(&self, queries: &QueryDb, tcx: TyCtxt) {
        let metadata = self.generate_metadata(tcx);
        let outpath = metadata_output_path(tcx);
        store_metadata(queries, &metadata, &outpath);
    }

    pub fn harness_model_path(&self, harness: Harness) -> Option<&PathBuf> {
        self.harness_info[&harness].goto_file.as_ref()
    }

    /// Generate [KaniMetadata] for the target crate.
    fn generate_metadata(&self, tcx: TyCtxt) -> KaniMetadata {
        let (proof_harnesses, test_harnesses) =
            self.harness_info.values().cloned().partition(|md| md.attributes.is_proof_harness());
        KaniMetadata {
            crate_name: self.crate_info.name.clone(),
            proof_harnesses,
            unsupported_features: vec![],
            test_harnesses,
            contracted_functions: gen_contracts_metadata(tcx),
            autoharness_skipped_fns: self.autoharness_skipped_fns.clone(),
        }
    }
}

fn stub_def(tcx: TyCtxt, def_id: DefId) -> FnDef {
    let ty_internal = tcx.type_of(def_id).instantiate_identity();
    let ty = rustc_internal::stable(ty_internal);
    if let TyKind::RigidTy(RigidTy::FnDef(def, _)) = ty.kind() {
        def
    } else {
        unreachable!("Expected stub function for `{:?}`, but found: {ty}", tcx.def_path(def_id))
    }
}

/// Group the harnesses by their stubs and contract usage.
fn group_by_stubs(
    tcx: TyCtxt,
    all_harnesses: &HashMap<Harness, HarnessMetadata>,
) -> Vec<CodegenUnit> {
    let mut per_stubs: HashMap<_, CodegenUnit> = HashMap::default();
    for (harness, metadata) in all_harnesses {
        let stub_ids = harness_stub_map(tcx, *harness, metadata);
        let contracts = extract_contracts(tcx, *harness, metadata);
        let stub_map = stub_ids
            .iter()
            .map(|(k, v)| (tcx.def_path_hash(*k), tcx.def_path_hash(*v)))
            .collect::<BTreeMap<_, _>>();
        let key = (contracts, stub_map);
        if let Some(unit) = per_stubs.get_mut(&key) {
            unit.harnesses.push(*harness);
        } else {
            let stubs = stub_ids
                .iter()
                .map(|(from, to)| (stub_def(tcx, *from), stub_def(tcx, *to)))
                .collect::<HashMap<_, _>>();
            let stubs = apply_transitivity(tcx, *harness, stubs);
            per_stubs.insert(key, CodegenUnit { stubs, harnesses: vec![*harness] });
        }
    }
    per_stubs.into_values().collect()
}

#[derive(Copy, Clone, Debug, Ord, PartialOrd, PartialEq, Eq, Hash)]
enum ContractUsage {
    Stub(usize),
    Check(usize),
}

/// Extract the contract related usages.
///
/// Note that any error interpreting the result is emitted, but we delay aborting, so we emit as
/// many errors as possible.
fn extract_contracts(
    tcx: TyCtxt,
    harness: Harness,
    metadata: &HarnessMetadata,
) -> BTreeSet<ContractUsage> {
    let def = harness.def;
    let mut result = BTreeSet::new();
    if let HarnessKind::ProofForContract { target_fn } = &metadata.attributes.kind {
        if let Ok(check_def) = expect_resolve_fn(tcx, def, target_fn, "proof_for_contract") {
            result.insert(ContractUsage::Check(check_def.def_id().to_index()));
        }
    }

    for stub in &metadata.attributes.verified_stubs {
        let Ok(stub_def) = expect_resolve_fn(tcx, def, stub, "stub_verified") else { continue };
        result.insert(ContractUsage::Stub(stub_def.def_id().to_index()));
    }

    result
}

/// Extract the filename for the metadata file.
fn metadata_output_path(tcx: TyCtxt) -> PathBuf {
    let filepath = tcx.output_filenames(()).path(OutputType::Object);
    let filename = filepath.as_path();
    filename.with_extension(ArtifactType::Metadata).to_path_buf()
}

/// Write the metadata to a file
fn store_metadata(queries: &QueryDb, metadata: &KaniMetadata, filename: &Path) {
    debug!(?filename, "store_metadata");
    let out_file = File::create(filename).unwrap();
    let writer = BufWriter::new(out_file);
    if queries.args().output_pretty_json {
        serde_json::to_writer_pretty(writer, &metadata).unwrap();
    } else {
        serde_json::to_writer(writer, &metadata).unwrap();
    }
}

/// Validate the unit configuration.
fn validate_units(tcx: TyCtxt, units: &[CodegenUnit]) {
    for unit in units {
        for (from, to) in &unit.stubs {
            // We use harness span since we don't keep the attribute span.
            let Err(msg) = check_compatibility(tcx, *from, *to) else { continue };
            let span = unit.harnesses.first().unwrap().def.span();
            tcx.dcx().span_err(rustc_internal::internal(tcx, span), msg);
        }
    }
    tcx.dcx().abort_if_errors();
}

/// Apply stub transitivity operations.
///
/// If `fn1` is stubbed by `fn2`, and `fn2` is stubbed by `fn3`, `f1` is in fact stubbed by `fn3`.
fn apply_transitivity(tcx: TyCtxt, harness: Harness, stubs: Stubs) -> Stubs {
    let mut new_stubs = Stubs::with_capacity(stubs.len());
    for (orig, new) in stubs.iter() {
        let mut new_fn = *new;
        let mut visited = HashSet::new();
        while let Some(stub) = stubs.get(&new_fn) {
            if !visited.insert(stub) {
                // Visiting the same stub, i.e. found cycle.
                let span = harness.def.span();
                tcx.dcx().span_err(
                    rustc_internal::internal(tcx, span),
                    format!(
                        "Cannot stub `{}`. Stub configuration for harness `{}` has a cycle",
                        orig.name(),
                        harness.def.name(),
                    ),
                );
                break;
            }
            new_fn = *stub;
        }
        new_stubs.insert(*orig, new_fn);
    }
    new_stubs
}

/// Fetch all manual harnesses (i.e., functions provided by the user) and generate their metadata
fn get_all_manual_harnesses(
    tcx: TyCtxt,
    base_filename: &Path,
) -> HashMap<Harness, HarnessMetadata> {
    let harnesses = filter_crate_items(tcx, |_, instance| is_proof_harness(tcx, instance));
    harnesses
        .into_iter()
        .map(|harness| {
            let metadata = gen_proof_metadata(tcx, harness, &base_filename);
            (harness, metadata)
        })
        .collect::<HashMap<_, _>>()
}

/// For each function eligible for automatic verification,
/// generate a harness Instance for it, then generate its metadata.
/// Note that the body of each harness instance is still the dummy body of `kani_harness_intrinsic`;
/// the AutomaticHarnessPass will later transform the bodies of these instances to actually verify the function.
fn get_all_automatic_harnesses(
    tcx: TyCtxt,
    verifiable_fns: Vec<Instance>,
    kani_harness_intrinsic: FnDef,
    base_filename: &Path,
) -> HashMap<Harness, HarnessMetadata> {
    verifiable_fns
        .into_iter()
        .map(|fn_to_verify| {
            // Set the generic arguments of the harness to be the function it is verifying
            // so that later, in AutomaticHarnessPass, we can retrieve the function to verify
            // and generate the harness body accordingly.
            let harness = Instance::resolve(
                kani_harness_intrinsic,
                &GenericArgs(vec![GenericArgKind::Type(fn_to_verify.ty())]),
            )
            .unwrap();
            let metadata = gen_automatic_proof_metadata(tcx, &base_filename, &fn_to_verify);
            (harness, metadata)
        })
        .collect::<HashMap<_, _>>()
}

/// Partition every function in the crate into (chosen, skipped), where `chosen` is a vector of the Instances for which we'll generate automatic harnesses,
/// and `skipped` is a map of function names to the reason why we skipped them.
fn automatic_harness_partition(
    tcx: TyCtxt,
    args: &Arguments,
    kani_any_def: FnDef,
) -> (Vec<Instance>, BTreeMap<String, AutoHarnessSkipReason>) {
    // If `filter_list` contains `name`, either as an exact match or a substring.
    let filter_contains = |name: &str, filter_list: &[String]| -> bool {
        filter_list.iter().any(|filter_name| name.contains(filter_name))
    };

    // If `func` is not eligible for an automatic harness, return the reason why; if it is eligible, return None.
    let skip_reason = |fn_item: CrateItem| -> Option<AutoHarnessSkipReason> {
        if KaniAttributes::for_def_id(tcx, fn_item.def_id()).is_kani_instrumentation() {
            return Some(AutoHarnessSkipReason::KaniImpl);
        }

        let instance = match Instance::try_from(fn_item) {
            Ok(inst) => inst,
            Err(_) => {
                return Some(AutoHarnessSkipReason::GenericFn);
            }
        };

        if !instance.has_body() {
            return Some(AutoHarnessSkipReason::NoBody);
        }

        let name = instance.name();
        let body = instance.body().unwrap();

        if is_proof_harness(tcx, instance)
            || name.contains("kani::Arbitrary")
            || name.contains("kani::Invariant")
        {
            return Some(AutoHarnessSkipReason::KaniImpl);
        }

        if (!args.autoharness_included_functions.is_empty()
            && !filter_contains(&name, &args.autoharness_included_functions))
            || (!args.autoharness_excluded_functions.is_empty()
                && filter_contains(&name, &args.autoharness_excluded_functions))
        {
            return Some(AutoHarnessSkipReason::UserFilter);
        }

        // Each argument of `instance` must implement Arbitrary.
        // Note that we've already filtered out generic functions, so we know that each of these arguments has a concrete type.
        let mut problematic_args = vec![];
        for (idx, arg) in body.arg_locals().iter().enumerate() {
            let kani_any_body =
                Instance::resolve(kani_any_def, &GenericArgs(vec![GenericArgKind::Type(arg.ty)]))
                    .unwrap()
                    .body()
                    .unwrap();

            let implements_arbitrary = if let TerminatorKind::Call { func, .. } =
                &kani_any_body.blocks[0].terminator.kind
            {
                if let Some((def, args)) = func.ty(body.arg_locals()).unwrap().kind().fn_def() {
                    Instance::resolve(def, &args).is_ok()
                } else {
                    false
                }
            } else {
                false
            };

            if !implements_arbitrary {
                // Find the name of the argument by referencing var_debug_info.
                // Note that enumerate() starts at 0, while StableMIR argument_index starts at 1, hence the idx+1.
                let arg_debug_info = body
                    .var_debug_info
                    .iter()
                    .find(|var| {
                        var.argument_index.is_some_and(|arg_idx| idx + 1 == usize::from(arg_idx))
                    })
                    .expect("Arguments should have corresponding var debug info");
                problematic_args.push(arg_debug_info.name.to_string())
            }
        }
        if !problematic_args.is_empty() {
            return Some(AutoHarnessSkipReason::MissingArbitraryImpl(problematic_args));
        }
        None
    };

    let mut chosen = vec![];
    let mut skipped = BTreeMap::new();

    // FIXME: ideally, this filter would be matches!(item.kind(), ItemKind::Fn), since that would allow us to generate harnesses for top-level closures,
    // c.f. https://github.com/model-checking/kani/issues/3832#issuecomment-2701671798.
    // Note that filtering closures out here is a UX choice: we could instead call skip_reason() on closures,
    // but the limitations in the linked issue would cause the user to be flooded with reports of "skipping" Kani instrumentation functions.
    // Instead, we just pretend closures don't exist in our reporting of what we did and did not verify, which has the downside of also ignoring the user's top-level closures, but those are rare.
    let crate_fns =
        stable_mir::all_local_items().into_iter().filter(|item| item.ty().kind().is_fn());

    for func in crate_fns {
        if let Some(reason) = skip_reason(func) {
            skipped.insert(func.name(), reason);
        } else {
            chosen.push(Instance::try_from(func).unwrap());
        }
    }

    (chosen, skipped)
}
