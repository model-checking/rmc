// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! This module contains code related to the MIR-to-MIR pass that performs the
//! stubbing of functions and methods. The primary function of the module is
//! `transform`, which takes the `DefId` of a function/method and returns the
//! body of its stub, if appropriate. The stub mapping it uses is set via rustc
//! arguments.

use lazy_static::lazy_static;
use regex::Regex;
use rustc_data_structures::fx::FxHashMap;
use rustc_hir::def_id::{DefId, LocalDefId};
use rustc_middle::{mir::Body, ty::TyCtxt};

/// Returns the new body of a function/method if it has been stubbed out;
/// otherwise, returns the old body.
pub fn transform<'tcx>(
    tcx: TyCtxt<'tcx>,
    def_id: DefId,
    old_body: &'tcx Body<'tcx>,
) -> &'tcx Body<'tcx> {
    if let Some(mapping) = get_stub_mapping(tcx) {
        let name = tcx.def_path_str(def_id);
        if let Some(replacement) = mapping.get(&name) {
            if let Some(replacement_id) = get_def_id(tcx, replacement) {
                let new_body = tcx.optimized_mir(replacement_id).clone();
                if check_compatibility(tcx, def_id, old_body, replacement_id, &new_body) {
                    return tcx.arena.alloc(new_body);
                }
            } else {
                tcx.sess
                    .span_err(tcx.def_span(def_id), format!("Unable to find stub: {replacement}"));
            };
        }
    }
    old_body
}

/// Checks whether the stub is compatible with the original function/method: do
/// the arities and types (of the parameters and return values) match up? This
/// does **NOT** check whether the type variables are constrained to implement
/// the same traits; trait mismatches are checked during monomorphization.
fn check_compatibility<'a, 'tcx>(
    tcx: TyCtxt,
    old_def_id: DefId,
    old_body: &'a Body<'tcx>,
    stub_def_id: DefId,
    stub_body: &'a Body<'tcx>,
) -> bool {
    // Check whether the arities match.
    if old_body.arg_count != stub_body.arg_count {
        tcx.sess.span_err(
            tcx.def_span(stub_def_id),
            format!(
                "Arity mismatch: original function/method `{}` takes {} argument(s), stub `{}` takes {}",
                tcx.def_path_str(old_def_id),
                old_body.arg_count,
                tcx.def_path_str(stub_def_id),
                stub_body.arg_count
            ),
        );
        return false;
    }
    // Check whether the types match. Index 0 refers to the returned value,
    // indices [1, `arg_count`] refer to the parameters.
    let mut matches = true;
    for i in 0..=old_body.arg_count {
        let old_arg = old_body.local_decls.get(i.into()).unwrap();
        let new_arg = stub_body.local_decls.get(i.into()).unwrap();
        if old_arg.ty != new_arg.ty {
            let prefix = if i == 0 {
                "Return type differs".to_string()
            } else {
                format!("Type of parameter {} differs", i - 1)
            };
            tcx.sess.span_err(
                new_arg.source_info.span,
                format!(
                    "{prefix}: stub `{}` has type `{}` where original function/method `{}` has type `{}`",
                    tcx.def_path_str(stub_def_id),
                    new_arg.ty,
                    tcx.def_path_str(old_def_id),
                    old_arg.ty
                ),
            );
            matches = false;
        }
    }
    matches
}

/// The prefix we will use when serializing the stub mapping as a rustc argument.
const RUSTC_ARG_PREFIX: &str = "kani_stubs=";

/// Serializes the stub mapping into a rustc argument.
pub fn mk_rustc_arg(stub_mapping: FxHashMap<String, String>) -> String {
    // Store our serialized mapping as a fake LLVM argument (safe to do since
    // LLVM will never see them).
    format!("-Cllvm-args='{RUSTC_ARG_PREFIX}{}'", serde_json::to_string(&stub_mapping).unwrap())
}

/// Deserializes the stub mapping from the rustc argument value.
fn deserialize_mapping(val: &str) -> FxHashMap<String, String> {
    serde_json::from_str(val).unwrap()
}

/// Retrieves the stub mapping from the compiler configuration.
fn get_stub_mapping(tcx: TyCtxt) -> Option<FxHashMap<String, String>> {
    // Use a static so that we compile the regex only once.
    lazy_static! {
        static ref RE: Regex = Regex::new(&format!("'{RUSTC_ARG_PREFIX}(.*)'")).unwrap();
    }
    for arg in &tcx.sess.opts.cg.llvm_args {
        if let Some(captures) = RE.captures(arg) {
            return Some(deserialize_mapping(captures.get(1).unwrap().as_str()));
        }
    }
    None
}

/// Tries to find the `DefId` of a function/method that matches the path `path`.
fn get_def_id(tcx: TyCtxt, path: &str) -> Option<DefId> {
    // TODO: This linear scan is potentially inefficient; we should find another
    // way of resolving the path to a `DefId`.
    // <https://github.com/model-checking/kani/issues/1894>
    tcx.iter_local_def_id().map(LocalDefId::to_def_id).find(|&id| tcx.def_path_str(id) == path)
}
