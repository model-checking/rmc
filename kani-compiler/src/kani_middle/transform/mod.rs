// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
//! This module is responsible for optimizing and instrumenting function bodies.
//!
//! We make transformations on bodies already monomorphized, which allow us to make stronger
//! decisions based on the instance types and constants.
//!
//! The main downside is that some transformation that don't depend on the specialized type may be
//! applied multiple times, one per specialization.
//!
//! Another downside is that these modifications cannot be applied to concrete playback, since they
//! are applied on the top of StableMIR body, which cannot be propagated back to rustc's backend.
//!
//! # Warn
//!
//! For all instrumentation passes, always use exhaustive matches to ensure soundness in case a new
//! case is added.
use crate::kani_middle::codegen_units::CodegenUnit;
use crate::kani_middle::reachability::CallGraph;
use crate::kani_middle::transform::body::CheckType;
use crate::kani_middle::transform::check_uninit::{DelayedUbPass, UninitPass};
use crate::kani_middle::transform::check_values::ValidValuePass;
use crate::kani_middle::transform::contracts::{AnyModifiesPass, FunctionWithContractPass};
use crate::kani_middle::transform::kani_intrinsics::IntrinsicGeneratorPass;
use crate::kani_middle::transform::stubs::{ExternFnStubPass, FnStubPass};
use crate::kani_queries::QueryDb;
use dump_mir_pass::DumpMirPass;
use rustc_middle::ty::TyCtxt;
use stable_mir::mir::mono::{Instance, MonoItem};
use stable_mir::mir::Body;
use std::collections::HashMap;
use std::fmt::Debug;

pub use internal_mir::RustcInternalMir;

pub(crate) mod body;
mod check_uninit;
mod check_values;
mod contracts;
mod dump_mir_pass;
mod internal_mir;
mod kani_intrinsics;
mod stubs;

/// Object used to retrieve a transformed instance body.
/// The transformations to be applied may be controlled by user options.
///
/// The order however is always the same, we run optimizations first, and instrument the code
/// after.
#[derive(Debug)]
pub struct BodyTransformation {
    /// The passes that may change the function body according to harness configuration.
    /// The stubbing passes should be applied before so user stubs take precedence.
    stub_passes: Vec<Box<dyn TransformPass>>,
    /// The passes that may add safety checks to the function body.
    inst_passes: Vec<Box<dyn TransformPass>>,
    /// Cache transformation results.
    cache: HashMap<Instance, TransformationResult>,
}

impl BodyTransformation {
    pub fn new(queries: &QueryDb, tcx: TyCtxt, unit: &CodegenUnit) -> Self {
        let mut transformer = BodyTransformation {
            stub_passes: vec![],
            inst_passes: vec![],
            cache: Default::default(),
        };
        let check_type = CheckType::new_assert_assume(tcx);
        transformer.add_pass(queries, FnStubPass::new(&unit.stubs));
        transformer.add_pass(queries, ExternFnStubPass::new(&unit.stubs));
        transformer.add_pass(queries, FunctionWithContractPass::new(tcx, &unit));
        // This has to come after the contract pass since we want this to only replace the closure
        // body that is relevant for this harness.
        transformer.add_pass(queries, AnyModifiesPass::new(tcx, &unit));
        transformer.add_pass(queries, ValidValuePass { check_type: check_type.clone() });
        // Putting `UninitPass` after `ValidValuePass` makes sure that the code generated by
        // `UninitPass` does not get unnecessarily instrumented by valid value checks. However, it
        // would also make sense to check that the values are initialized before checking their
        // validity. In the future, it would be nice to have a mechanism to skip automatically
        // generated code for future instrumentation passes.
        transformer.add_pass(
            queries,
            UninitPass {
                // Since this uses demonic non-determinism under the hood, should not assume the assertion.
                check_type: CheckType::new_assert(tcx),
                mem_init_fn_cache: HashMap::new(),
            },
        );
        transformer.add_pass(
            queries,
            IntrinsicGeneratorPass {
                check_type,
                mem_init_fn_cache: HashMap::new(),
                arguments: queries.args().clone(),
            },
        );
        transformer
    }

    /// Retrieve the body of an instance. This does not apply global passes, but will retrieve the
    /// body after global passes running if they were previously applied.
    ///
    /// Note that this assumes that the instance does have a body since existing consumers already
    /// assume that. Use `instance.has_body()` to check if an instance has a body.
    pub fn body(&mut self, tcx: TyCtxt, instance: Instance) -> Body {
        match self.cache.get(&instance) {
            Some(TransformationResult::Modified(body)) => body.clone(),
            Some(TransformationResult::NotModified) => instance.body().unwrap(),
            None => {
                let mut body = instance.body().unwrap();
                let mut modified = false;
                for pass in self.stub_passes.iter_mut().chain(self.inst_passes.iter_mut()) {
                    let result = pass.transform(tcx, body, instance);
                    modified |= result.0;
                    body = result.1;
                }

                let result = if modified {
                    TransformationResult::Modified(body.clone())
                } else {
                    TransformationResult::NotModified
                };
                self.cache.insert(instance, result);
                body
            }
        }
    }

    fn add_pass<P: TransformPass + 'static>(&mut self, query_db: &QueryDb, pass: P) {
        if pass.is_enabled(&query_db) {
            match P::transformation_type() {
                TransformationType::Instrumentation => self.inst_passes.push(Box::new(pass)),
                TransformationType::Stubbing => self.stub_passes.push(Box::new(pass)),
            }
        }
    }
}

/// The type of transformation that a pass may perform.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) enum TransformationType {
    /// Should only add assertion checks to ensure the program is correct.
    Instrumentation,
    /// Apply some sort of stubbing.
    Stubbing,
}

/// A trait to represent transformation passes that can be used to modify the body of a function.
pub(crate) trait TransformPass: Debug {
    /// The type of transformation that this pass implements.
    fn transformation_type() -> TransformationType
    where
        Self: Sized;

    fn is_enabled(&self, query_db: &QueryDb) -> bool
    where
        Self: Sized;

    /// Run a transformation pass in the function body.
    fn transform(&mut self, tcx: TyCtxt, body: Body, instance: Instance) -> (bool, Body);
}

/// A trait to represent transformation passes that operate on the whole codegen unit.
pub(crate) trait GlobalPass: Debug {
    fn is_enabled(&self, query_db: &QueryDb) -> bool
    where
        Self: Sized;

    /// Run a transformation pass on the whole codegen unit.
    fn transform(
        &mut self,
        tcx: TyCtxt,
        call_graph: &CallGraph,
        starting_items: &[MonoItem],
        instances: Vec<Instance>,
        transformer: &mut BodyTransformation,
    );
}

/// The transformation result.
/// We currently only cache the body of functions that were instrumented.
#[derive(Clone, Debug)]
enum TransformationResult {
    Modified(Body),
    NotModified,
}

pub struct GlobalPasses {
    /// The passes that operate on the whole codegen unit, they run after all previous passes are
    /// done.
    global_passes: Vec<Box<dyn GlobalPass>>,
}

impl GlobalPasses {
    pub fn new(queries: &QueryDb, tcx: TyCtxt) -> Self {
        let mut global_passes = GlobalPasses { global_passes: vec![] };
        global_passes.add_global_pass(queries, DelayedUbPass::new(CheckType::new_assert(tcx)));
        global_passes.add_global_pass(queries, DumpMirPass::new(tcx));
        global_passes
    }

    fn add_global_pass<P: GlobalPass + 'static>(&mut self, query_db: &QueryDb, pass: P) {
        if pass.is_enabled(&query_db) {
            self.global_passes.push(Box::new(pass))
        }
    }

    /// Run all global passes and store the results in a cache that can later be queried by `body`.
    pub fn run_global_passes(
        &mut self,
        transformer: &mut BodyTransformation,
        tcx: TyCtxt,
        starting_items: &[MonoItem],
        instances: Vec<Instance>,
        call_graph: CallGraph,
    ) {
        for global_pass in self.global_passes.iter_mut() {
            global_pass.transform(tcx, &call_graph, starting_items, instances.clone(), transformer);
        }
    }
}
