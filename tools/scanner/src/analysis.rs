// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Provide passes that perform intra-function analysis on the crate under compilation

use crate::info;
use csv::WriterBuilder;
use graph_cycles::Cycles;
use petgraph::graph::Graph;
use serde::{Serialize, Serializer, ser::SerializeStruct};
use stable_mir::mir::mono::Instance;
use stable_mir::mir::visit::{Location, PlaceContext, PlaceRef};
use stable_mir::mir::{
    BasicBlock, Body, CastKind, MirVisitor, Mutability, NonDivergingIntrinsic, ProjectionElem,
    Rvalue, Safety, Statement, StatementKind, Terminator, TerminatorKind,
};
use stable_mir::ty::{Abi, AdtDef, AdtKind, FnDef, GenericArgs, MirConst, RigidTy, Ty, TyKind};
use stable_mir::visitor::{Visitable, Visitor};
use stable_mir::{CrateDef, CrateItem};
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct OverallStats {
    /// The key and value of each counter.
    pub counters: Vec<(&'static str, usize)>,
    /// TODO: Group stats per function.
    fn_stats: HashMap<CrateItem, FnStats>,
}

#[derive(Clone, Debug, Serialize)]
struct FnStats {
    name: String,
    is_unsafe: Option<bool>,
    has_unsafe_ops: Option<bool>,
    has_unsupported_input: Option<bool>,
    has_loop: Option<bool>,
    /// How many degrees of separation to unsafe code if any?
    /// - `None` if this function is indeed safe.
    /// - 0 if this function contains unsafe code (including invoking unsafe fns).
    /// - 1 if this function calls a safe abstraction.
    /// - 2+ if this function calls other functions that call safe abstractions.
    unsafe_distance: Option<usize>,
}

impl FnStats {
    fn new(fn_item: CrateItem) -> FnStats {
        FnStats {
            name: fn_item.name(),
            is_unsafe: None,
            has_unsafe_ops: None,
            has_unsupported_input: None,
            // TODO: Implement this.
            has_loop: None,
            unsafe_distance: None,
        }
    }
}

impl OverallStats {
    pub fn new() -> OverallStats {
        let all_items = stable_mir::all_local_items();
        let fn_stats: HashMap<_, _> = all_items
            .into_iter()
            .filter_map(|item| item.ty().kind().is_fn().then_some((item, FnStats::new(item))))
            .collect();
        let counters = vec![("total_fns", fn_stats.len())];
        OverallStats { counters, fn_stats }
    }

    pub fn store_csv(&self, base_path: PathBuf, file_stem: &str) {
        let filename = format!("{}_overall", file_stem);
        let mut out_path = base_path.parent().map_or(PathBuf::default(), Path::to_path_buf);
        out_path.set_file_name(filename);
        dump_csv(out_path, &self.counters);

        let filename = format!("{}_functions", file_stem);
        let mut out_path = base_path.parent().map_or(PathBuf::default(), Path::to_path_buf);
        out_path.set_file_name(filename);
        dump_csv(out_path, &self.fn_stats.values().collect::<Vec<_>>());
    }

    /// Iterate over all functions defined in this crate and log generic vs monomorphic.
    pub fn generic_fns(&mut self) {
        let all_items = stable_mir::all_local_items();
        let fn_items =
            all_items.into_iter().filter(|item| item.ty().kind().is_fn()).collect::<Vec<_>>();
        let (mono_fns, generics) = fn_items
            .iter()
            .partition::<Vec<&CrateItem>, _>(|fn_item| Instance::try_from(**fn_item).is_ok());
        self.counters
            .extend_from_slice(&[("generic_fns", generics.len()), ("mono_fns", mono_fns.len())]);
    }

    /// Iterate over all functions defined in this crate and log safe vs unsafe.
    pub fn safe_fns(&mut self, _base_filename: PathBuf) {
        let all_items = stable_mir::all_local_items();
        let (unsafe_fns, safe_fns) = all_items
            .into_iter()
            .filter_map(|item| {
                let kind = item.ty().kind();
                if !kind.is_fn() {
                    return None;
                };
                let fn_sig = kind.fn_sig().unwrap();
                let is_unsafe = fn_sig.skip_binder().safety == Safety::Unsafe;
                self.fn_stats.get_mut(&item).unwrap().is_unsafe = Some(is_unsafe);
                Some((item, is_unsafe))
            })
            .partition::<Vec<(CrateItem, bool)>, _>(|(_, is_unsafe)| *is_unsafe);
        self.counters
            .extend_from_slice(&[("safe_fns", safe_fns.len()), ("unsafe_fns", unsafe_fns.len())]);
    }

    /// Iterate over all functions defined in this crate and log the inputs.
    pub fn supported_inputs(&mut self, filename: PathBuf) {
        let all_items = stable_mir::all_local_items();
        let (supported, unsupported) = all_items
            .into_iter()
            .filter_map(|item| {
                let kind = item.ty().kind();
                if !kind.is_fn() {
                    return None;
                };
                let fn_sig = kind.fn_sig().unwrap();
                let props = FnInputProps::new(item.name()).collect(fn_sig.skip_binder().inputs());
                self.fn_stats.get_mut(&item).unwrap().has_unsupported_input =
                    Some(!props.is_supported());
                Some(props)
            })
            .partition::<Vec<_>, _>(|props| props.is_supported());
        self.counters.extend_from_slice(&[
            ("supported_inputs", supported.len()),
            ("unsupported_inputs", unsupported.len()),
        ]);
        dump_csv(filename, &unsupported);
    }

    /// Iterate over all functions defined in this crate and log any unsafe operation.
    pub fn unsafe_operations(&mut self, filename: PathBuf) {
        let all_items = stable_mir::all_local_items();
        let (has_unsafe, no_unsafe) = all_items
            .into_iter()
            .filter_map(|item| {
                let kind = item.ty().kind();
                if !kind.is_fn() {
                    return None;
                };
                let unsafe_ops = FnUnsafeOperations::new(item.name()).collect(&item.body());
                let fn_sig = kind.fn_sig().unwrap();
                let is_unsafe = fn_sig.skip_binder().safety == Safety::Unsafe;
                self.fn_stats.get_mut(&item).unwrap().has_unsafe_ops =
                    Some(unsafe_ops.has_unsafe());
                Some((is_unsafe, unsafe_ops))
            })
            .partition::<Vec<_>, _>(|(_, props)| props.has_unsafe());
        self.counters.extend_from_slice(&[
            ("has_unsafe_ops", has_unsafe.len()),
            ("no_unsafe_ops", no_unsafe.len()),
            ("safe_abstractions", has_unsafe.iter().filter(|(is_unsafe, _)| !is_unsafe).count()),
        ]);
        dump_csv(filename, &has_unsafe.into_iter().map(|(_, props)| props).collect::<Vec<_>>());
    }

    /// Iterate over all functions defined in this crate and log any loop / "hidden" loop.
    ///
    /// A hidden loop is a call to a iterator function that has a loop inside.
    pub fn loops(&mut self, filename: PathBuf) {
        let all_items = stable_mir::all_local_items();
        let (has_loops, no_loops) = all_items
            .clone()
            .into_iter()
            .filter_map(|item| {
                let kind = item.ty().kind();
                if !kind.is_fn() {
                    return None;
                };
                Some(FnLoops::new(item.name()).collect(&item.body()))
            })
            .partition::<Vec<_>, _>(|props| props.has_loops());

        let (has_iterators, no_iterators) = all_items
            .clone()
            .into_iter()
            .filter_map(|item| {
                let kind = item.ty().kind();
                if !kind.is_fn() {
                    return None;
                };
                Some(FnLoops::new(item.name()).collect(&item.body()))
            })
            .partition::<Vec<_>, _>(|props| props.has_iterators());

        let (has_either, _) = all_items
            .into_iter()
            .filter_map(|item| {
                let kind = item.ty().kind();
                if !kind.is_fn() {
                    return None;
                };
                Some(FnLoops::new(item.name()).collect(&item.body()))
            })
            .partition::<Vec<_>, _>(|props| props.has_iterators() || props.has_loops());

        self.counters.extend_from_slice(&[
            ("has_loops", has_loops.len()),
            ("no_loops", no_loops.len()),
            ("has_iterators", has_iterators.len()),
            ("no_iterators", no_iterators.len()),
        ]);
        dump_csv(filename, &has_either);
    }

    /// Create a callgraph for this crate and try to find recursive calls.
    pub fn recursion(&mut self, filename: PathBuf) {
        let all_items = stable_mir::all_local_items();
        let recursions = Recursion::collect(&all_items);
        self.counters.extend_from_slice(&[
            ("with_recursion", recursions.with_recursion.len()),
            ("recursive_fns", recursions.recursive_fns.len()),
        ]);
        dump_csv(
            filename,
            &recursions
                .with_recursion
                .iter()
                .map(|def| {
                    (
                        def.name(),
                        if recursions.recursive_fns.contains(&def) { "recursive" } else { "" },
                    )
                })
                .collect::<Vec<_>>(),
        );
    }
}

macro_rules! fn_props {
    ($(#[$attr:meta])*
    $vis:vis struct $name:ident {
        $(
            $(#[$prop_attr:meta])*
            $prop:ident,
        )+
    }) => {
        #[derive(Debug)]
        $vis struct $name {
            fn_name: String,
            $($(#[$prop_attr])* $prop: usize,)+
        }

        impl $name {
            $vis const fn num_props() -> usize {
                [$(stringify!($prop),)+].len()
            }

            $vis fn new(fn_name: String) -> Self {
                Self { fn_name, $($prop: 0,)+}
            }
        }

        /// Need to manually implement this, since CSV serializer does not support map (i.e.: flatten).
        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                let mut state = serializer.serialize_struct("FnInputProps", Self::num_props())?;
                state.serialize_field("fn_name", &self.fn_name)?;
                $(state.serialize_field(stringify!($prop), &self.$prop)?;)+
                state.end()
            }
        }
    };
}

fn_props! {
    struct FnInputProps {
        boxes,
        closures,
        coroutines,
        floats,
        fn_defs,
        fn_ptrs,
        generics,
        interior_muts,
        raw_ptrs,
        recursive_types,
        mut_refs,
        simd,
        unions,
    }
}

impl FnInputProps {
    pub fn collect(mut self, inputs: &[Ty]) -> FnInputProps {
        for input in inputs {
            let mut visitor = TypeVisitor { metrics: &mut self, visited: HashSet::new() };
            let _ = visitor.visit_ty(input);
        }
        self
    }

    pub fn is_supported(&self) -> bool {
        (self.closures
            + self.coroutines
            + self.floats
            + self.fn_defs
            + self.fn_ptrs
            + self.interior_muts
            + self.raw_ptrs
            + self.recursive_types
            + self.mut_refs)
            == 0
    }
}

struct TypeVisitor<'a> {
    metrics: &'a mut FnInputProps,
    visited: HashSet<Ty>,
}

impl TypeVisitor<'_> {
    pub fn visit_variants(&mut self, def: AdtDef, _args: &GenericArgs) -> ControlFlow<()> {
        for variant in def.variants_iter() {
            for field in variant.fields() {
                self.visit_ty(&field.ty())?
            }
        }
        ControlFlow::Continue(())
    }
}

impl Visitor for TypeVisitor<'_> {
    type Break = ();

    fn visit_ty(&mut self, ty: &Ty) -> ControlFlow<Self::Break> {
        if self.visited.contains(ty) {
            self.metrics.recursive_types += 1;
            ControlFlow::Continue(())
        } else {
            self.visited.insert(*ty);
            let kind = ty.kind();
            match kind {
                TyKind::Alias(..) => {}
                TyKind::Param(_) => self.metrics.generics += 1,
                TyKind::RigidTy(rigid) => match rigid {
                    RigidTy::Coroutine(..) => self.metrics.coroutines += 1,
                    RigidTy::Closure(..) => self.metrics.closures += 1,
                    RigidTy::FnDef(..) => self.metrics.fn_defs += 1,
                    RigidTy::FnPtr(..) => self.metrics.fn_ptrs += 1,
                    RigidTy::Float(..) => self.metrics.floats += 1,
                    RigidTy::RawPtr(..) => self.metrics.raw_ptrs += 1,
                    RigidTy::Ref(_, _, Mutability::Mut) => self.metrics.mut_refs += 1,
                    RigidTy::Adt(def, args) => match def.kind() {
                        AdtKind::Union => self.metrics.unions += 1,
                        _ => {
                            let name = def.name();
                            if def.is_box() {
                                self.metrics.boxes += 1;
                            } else if name.ends_with("UnsafeCell") {
                                self.metrics.interior_muts += 1;
                            } else {
                                self.visit_variants(def, &args)?;
                            }
                        }
                    },
                    _ => {}
                },
                kind => unreachable!("Expected rigid type, but found: {kind:?}"),
            }
            ty.super_visit(self)
        }
    }
}

pub(crate) fn dump_csv<T: Serialize>(mut out_path: PathBuf, data: &[T]) {
    out_path.set_extension("csv");
    info(format!("Write file: {out_path:?}"));
    let mut writer = WriterBuilder::new().delimiter(b';').from_path(&out_path).unwrap();
    for d in data {
        writer.serialize(d).unwrap();
    }
}

fn_props! {
    pub struct FnUnsafeOperations {
        inline_assembly,
        /// Dereference a raw pointer.
        /// This is also counted when we access a static variable since it gets translated to a raw pointer.
        unsafe_dereference,
        /// Call an unsafe function or method including C-FFI.
        unsafe_call,
        /// Access or modify a mutable static variable.
        unsafe_static_access,
        /// Access fields of unions.
        unsafe_union_access,
        /// Invoke external functions (this is a subset of `unsafe_call`.
        extern_call,
        /// Transmute operations.
        transmute,
        /// Cast raw pointer to reference.
        unsafe_cast,
    }
}

impl FnUnsafeOperations {
    pub fn collect(self, body: &Body) -> FnUnsafeOperations {
        let mut visitor = BodyVisitor { props: self, body };
        visitor.visit_body(body);
        visitor.props
    }

    pub fn has_unsafe(&self) -> bool {
        (self.inline_assembly
            + self.unsafe_static_access
            + self.unsafe_dereference
            + self.unsafe_union_access
            + self.unsafe_call)
            > 0
    }
}

struct BodyVisitor<'a> {
    props: FnUnsafeOperations,
    body: &'a Body,
}

impl MirVisitor for BodyVisitor<'_> {
    fn visit_terminator(&mut self, term: &Terminator, location: Location) {
        match &term.kind {
            TerminatorKind::Call { func, .. } => {
                let TyKind::RigidTy(RigidTy::FnDef(fn_def, _)) =
                    func.ty(self.body.locals()).unwrap().kind()
                else {
                    return self.super_terminator(term, location);
                };
                let fn_sig = fn_def.fn_sig().skip_binder();
                if fn_sig.safety == Safety::Unsafe {
                    self.props.unsafe_call += 1;
                    if !matches!(
                        fn_sig.abi,
                        Abi::Rust | Abi::RustCold | Abi::RustCall | Abi::RustIntrinsic
                    ) && !fn_def.has_body()
                    {
                        self.props.extern_call += 1;
                    }
                }
            }
            TerminatorKind::InlineAsm { .. } => self.props.inline_assembly += 1,
            _ => { /* safe */ }
        }
        self.super_terminator(term, location)
    }

    fn visit_rvalue(&mut self, rvalue: &Rvalue, location: Location) {
        if let Rvalue::Cast(cast_kind, operand, ty) = rvalue {
            match cast_kind {
                CastKind::Transmute => {
                    self.props.transmute += 1;
                }
                _ => {
                    let operand_ty = operand.ty(self.body.locals()).unwrap();
                    if ty.kind().is_ref() && operand_ty.kind().is_raw_ptr() {
                        self.props.unsafe_cast += 1;
                    }
                }
            }
        };
        self.super_rvalue(rvalue, location);
    }

    fn visit_statement(&mut self, stmt: &Statement, location: Location) {
        if matches!(
            &stmt.kind,
            StatementKind::Intrinsic(NonDivergingIntrinsic::CopyNonOverlapping(_))
        ) {
            // Treat this as invoking the copy intrinsic.
            self.props.unsafe_call += 1;
        }
        self.super_statement(stmt, location)
    }

    fn visit_projection_elem(
        &mut self,
        place: PlaceRef,
        elem: &ProjectionElem,
        ptx: PlaceContext,
        location: Location,
    ) {
        match elem {
            ProjectionElem::Deref => {
                if place.ty(self.body.locals()).unwrap().kind().is_raw_ptr() {
                    self.props.unsafe_dereference += 1;
                }
            }
            ProjectionElem::Field(_, ty) => {
                if ty.kind().is_union() {
                    self.props.unsafe_union_access += 1;
                }
            }
            ProjectionElem::Downcast(_) => {}
            ProjectionElem::OpaqueCast(_) => {}
            ProjectionElem::Subtype(_) => {}
            ProjectionElem::Index(_)
            | ProjectionElem::ConstantIndex { .. }
            | ProjectionElem::Subslice { .. } => { /* safe */ }
        }
        self.super_projection_elem(elem, ptx, location)
    }

    fn visit_mir_const(&mut self, constant: &MirConst, location: Location) {
        if constant.ty().kind().is_raw_ptr() {
            self.props.unsafe_static_access += 1;
        }
        self.super_mir_const(constant, location)
    }
}

fn_props! {
    struct FnLoops {
        iterators,
        loops,
        // TODO: Collect nested loops.
        nested_loops,
    }
}

impl FnLoops {
    pub fn collect(self, body: &Body) -> FnLoops {
        let mut visitor =
            IteratorVisitor { props: self, body, graph: Vec::new(), current_bbidx: 0 };
        visitor.visit_body(body);
        visitor.props
    }

    pub fn has_loops(&self) -> bool {
        (self.loops + self.nested_loops) > 0
    }

    pub fn has_iterators(&self) -> bool {
        (self.iterators) > 0
    }
}

/// Try to find hidden loops by looking for calls to Iterator functions that has a loop in them.
///
/// Note that this will not find a loop, if the iterator is called inside a closure.
/// Run with -C opt-level 2 to help with this issue (i.e.: inline).
struct IteratorVisitor<'a> {
    props: FnLoops,
    body: &'a Body,
    graph: Vec<(u32, u32)>,
    current_bbidx: u32,
}

impl MirVisitor for IteratorVisitor<'_> {
    fn visit_body(&mut self, body: &Body) {
        // First visit the body to build the control flow graph
        self.super_body(body);
        // Build the petgraph from the adj vec
        let g = Graph::<(), ()>::from_edges(self.graph.clone());
        self.props.loops += g.cycles().len();
    }

    fn visit_basic_block(&mut self, bb: &BasicBlock) {
        self.current_bbidx = self.body.blocks.iter().position(|b| *b == *bb).unwrap() as u32;
        self.super_basic_block(bb);
    }

    fn visit_terminator(&mut self, term: &Terminator, location: Location) {
        // Add edges between basic block into the adj table
        let successors = term.kind.successors();
        for target in successors {
            self.graph.push((self.current_bbidx, target as u32));
        }

        if let TerminatorKind::Call { func, .. } = &term.kind {
            let kind = func.ty(self.body.locals()).unwrap().kind();
            // Check if the target is a visited block.

            // Check if the call is an iterator function that contains loops.
            if let TyKind::RigidTy(RigidTy::FnDef(def, _)) = kind {
                let fullname = def.name();
                let names = fullname.split("::").collect::<Vec<_>>();
                if let [.., s_last, last] = names.as_slice() {
                    if *s_last == "Iterator"
                        && [
                            "for_each",
                            "collect",
                            "advance_by",
                            "all",
                            "any",
                            "partition",
                            "partition_in_place",
                            "fold",
                            "try_fold",
                            "spec_fold",
                            "spec_try_fold",
                            "try_for_each",
                            "for_each",
                            "try_reduce",
                            "reduce",
                            "find",
                            "find_map",
                            "try_find",
                            "position",
                            "rposition",
                            "nth",
                            "count",
                            "last",
                            "find",
                        ]
                        .contains(last)
                    {
                        self.props.iterators += 1;
                    }
                }
            }
        }

        self.super_terminator(term, location)
    }
}

#[derive(Debug, Default)]
struct Recursion {
    /// Collect the functions that may lead to a recursion loop.
    /// I.e., for the following control flow graph:
    /// ```dot
    /// A -> B
    /// B -> C
    /// C -> [B, D]
    /// ```
    /// this field value would contain A, B, and C since they can all lead to a recursion.
    with_recursion: HashSet<FnDef>,
    /// Collect the functions that are part of a recursion loop.
    /// For the following control flow graph:
    /// ```dot
    /// A -> [B, C]
    /// B -> B
    /// C -> D
    /// D -> [C, E]
    /// ```
    /// The recursive functions would be B, C, and D.
    recursive_fns: HashSet<FnDef>,
}

impl Recursion {
    pub fn collect<'a>(items: impl IntoIterator<Item = &'a CrateItem>) -> Recursion {
        let call_graph = items
            .into_iter()
            .filter_map(|item| {
                if let TyKind::RigidTy(RigidTy::FnDef(def, _)) = item.ty().kind() {
                    let body = item.body();
                    let mut visitor = FnCallVisitor { body: &body, fns: vec![] };
                    visitor.visit_body(&body);
                    Some((def, visitor.fns))
                } else {
                    None
                }
            })
            .collect::<HashMap<_, _>>();
        let mut recursions = Recursion::default();
        recursions.analyze(call_graph);
        recursions
    }

    /// DFS post-order traversal to collect all loops in our control flow graph.
    /// We only include direct call recursions which can only happen within a crate.
    ///
    /// # How it works
    ///
    /// Given a call graph, [(fn_def, [fn_def]*)]*, enqueue all existing nodes together with the
    /// graph distance.
    /// Keep track of the current path and the visiting status of each node.
    /// For those that we have visited once, store whether a loop is reachable from them.
    fn analyze(&mut self, call_graph: HashMap<FnDef, Vec<FnDef>>) {
        #[derive(Copy, Clone, PartialEq, Eq)]
        enum Status {
            ToVisit,
            Visiting,
            Visited,
        }
        let mut visit_status = HashMap::<FnDef, Status>::new();
        let mut queue: Vec<_> = call_graph.keys().map(|node| (*node, 0)).collect();
        let mut path: Vec<FnDef> = vec![];
        while let Some((next, level)) = queue.last().copied() {
            match visit_status.get(&next).unwrap_or(&Status::ToVisit) {
                Status::ToVisit => {
                    assert_eq!(path.len(), level);
                    path.push(next);
                    visit_status.insert(next, Status::Visiting);
                    let next_level = level + 1;
                    if let Some(callees) = call_graph.get(&next) {
                        queue.extend(callees.iter().map(|callee| (*callee, next_level)));
                    }
                }
                Status::Visiting => {
                    if level < path.len() {
                        // We have visited all callees in this node.
                        visit_status.insert(next, Status::Visited);
                        path.pop();
                    } else {
                        // Found a loop.
                        let mut in_loop = false;
                        for def in &path {
                            in_loop |= *def == next;
                            if in_loop {
                                self.recursive_fns.insert(*def);
                            }
                            self.with_recursion.insert(*def);
                        }
                    }
                    queue.pop();
                }
                Status::Visited => {
                    queue.pop();
                    if self.with_recursion.contains(&next) {
                        self.with_recursion.extend(&path);
                    }
                }
            }
        }
    }
}

pub struct FnCallVisitor<'a> {
    pub body: &'a Body,
    pub fns: Vec<FnDef>,
}

impl MirVisitor for FnCallVisitor<'_> {
    fn visit_terminator(&mut self, term: &Terminator, location: Location) {
        if let TerminatorKind::Call { func, .. } = &term.kind {
            let kind = func.ty(self.body.locals()).unwrap().kind();
            if let TyKind::RigidTy(RigidTy::FnDef(def, _)) = kind {
                self.fns.push(def);
            }
        }
        self.super_terminator(term, location)
    }
}
