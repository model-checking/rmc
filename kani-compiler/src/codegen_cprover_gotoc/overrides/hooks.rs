// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module contains various codegen hooks for functions that require special handling.
//!
//! E.g.: Functions in the Kani library that generate assumptions or symbolic variables.
//!
//! It would be too nasty if we spread around these sort of undocumented hooks in place, so
//! this module addresses this issue.

use crate::codegen_cprover_gotoc::codegen::PropertyClass;
use crate::codegen_cprover_gotoc::GotocCtx;
use crate::unwrap_or_return_codegen_unimplemented_stmt;
use cbmc::goto_program::{BuiltinFn, Expr, Location, Stmt, Type};
use rustc_middle::mir::{BasicBlock, Place};
use rustc_middle::ty::print::with_no_trimmed_paths;
use rustc_middle::ty::{Instance, TyCtxt};
use rustc_span::Span;
use std::rc::Rc;
use tracing::debug;

pub trait GotocHook<'tcx> {
    /// if the hook applies, it means the codegen would do something special to it
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool;
    /// the handler for codegen
    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        instance: Instance<'tcx>,
        fargs: Vec<Expr>,
        assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt;
}

fn matches_function(tcx: TyCtxt, instance: Instance, attr_name: &str) -> bool {
    let attr_sym = rustc_span::symbol::Symbol::intern(attr_name);
    if let Some(attr_id) = tcx.all_diagnostic_items(()).name_to_id.get(&attr_sym) {
        if instance.def.def_id() == *attr_id {
            debug!("matched: {:?} {:?}", attr_id, attr_sym);
            return true;
        }
    }
    false
}

/// A hook for Kani's `cover` function (declared in `library/kani/src/lib.rs`).
/// The function takes two arguments: a condition expression (bool) and a
/// message (&'static str).
/// The hook codegens the function as a cover property that checks whether the
/// condition is satisfiable. Unlike assertions, cover properties currently do
/// not have an impact on verification success or failure. See
/// <https://github.com/model-checking/kani/blob/main/rfc/src/rfcs/0003-cover-statement.md>
/// for more details.
struct Cover;
impl<'tcx> GotocHook<'tcx> for Cover {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        matches_function(tcx, instance, "KaniCover")
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        _instance: Instance<'tcx>,
        mut fargs: Vec<Expr>,
        _assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let cond = fargs.remove(0).cast_to(Type::bool());
        let msg = fargs.remove(0);
        let msg = tcx.extract_const_message(&msg).unwrap();
        let target = target.unwrap();
        let caller_loc = tcx.codegen_caller_span(&span);

        let (msg, reach_stmt) = tcx.codegen_reachability_check(msg, span);

        Stmt::block(
            vec![
                reach_stmt,
                tcx.codegen_cover(cond, &msg, span),
                Stmt::goto(tcx.current_fn().find_label(&target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct Assume;
impl<'tcx> GotocHook<'tcx> for Assume {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        matches_function(tcx, instance, "KaniAssume")
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        _instance: Instance<'tcx>,
        mut fargs: Vec<Expr>,
        _assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        assert_eq!(fargs.len(), 1);
        let cond = fargs.remove(0).cast_to(Type::bool());
        let target = target.unwrap();
        let loc = tcx.codegen_span_option(span);

        Stmt::block(
            vec![
                tcx.codegen_assume(cond, loc),
                Stmt::goto(tcx.current_fn().find_label(&target), loc),
            ],
            loc,
        )
    }
}

struct Assert;
impl<'tcx> GotocHook<'tcx> for Assert {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        matches_function(tcx, instance, "KaniAssert")
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        _instance: Instance<'tcx>,
        mut fargs: Vec<Expr>,
        _assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let cond = fargs.remove(0).cast_to(Type::bool());
        let msg = fargs.remove(0);
        let msg = tcx.extract_const_message(&msg).unwrap();
        let target = target.unwrap();
        let caller_loc = tcx.codegen_caller_span(&span);

        let (msg, reach_stmt) = tcx.codegen_reachability_check(msg, span);

        // Since `cond` might have side effects, assign it to a temporary
        // variable so that it's evaluated once, then assert and assume it
        // TODO: I don't think `cond` can have side effects, this is MIR, it's going to be temps
        let (tmp, decl) = tcx.decl_temp_variable(cond.typ().clone(), Some(cond), caller_loc);
        Stmt::block(
            vec![
                reach_stmt,
                decl,
                tcx.codegen_assert_assume(tmp, PropertyClass::Assertion, &msg, caller_loc),
                Stmt::goto(tcx.current_fn().find_label(&target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct Nondet;

impl<'tcx> GotocHook<'tcx> for Nondet {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        matches_function(tcx, instance, "KaniAnyRaw")
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        _instance: Instance<'tcx>,
        fargs: Vec<Expr>,
        assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        assert!(fargs.is_empty());
        let loc = tcx.codegen_span_option(span);
        let target = target.unwrap();
        let pt = tcx.place_ty(&assign_to);
        if pt.is_unit() {
            Stmt::goto(tcx.current_fn().find_label(&target), loc)
        } else {
            let pe =
                unwrap_or_return_codegen_unimplemented_stmt!(tcx, tcx.codegen_place(&assign_to))
                    .goto_expr;
            Stmt::block(
                vec![
                    pe.assign(tcx.codegen_ty(pt).nondet(), loc),
                    Stmt::goto(tcx.current_fn().find_label(&target), loc),
                ],
                loc,
            )
        }
    }
}

struct Panic;

impl<'tcx> GotocHook<'tcx> for Panic {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        let def_id = instance.def.def_id();
        Some(def_id) == tcx.lang_items().panic_fn()
            || Some(def_id) == tcx.lang_items().panic_display()
            || Some(def_id) == tcx.lang_items().panic_fmt()
            || Some(def_id) == tcx.lang_items().begin_panic_fn()
            || matches_function(tcx, instance, "KaniPanic")
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        _instance: Instance<'tcx>,
        fargs: Vec<Expr>,
        _assign_to: Place<'tcx>,
        _target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        tcx.codegen_panic(span, fargs)
    }
}

struct RustAlloc;
// Removing this hook causes regression failures.
// https://github.com/model-checking/kani/issues/1170
impl<'tcx> GotocHook<'tcx> for RustAlloc {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        let full_name = with_no_trimmed_paths!(tcx.def_path_str(instance.def_id()));
        full_name == "alloc::alloc::exchange_malloc"
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        instance: Instance<'tcx>,
        mut fargs: Vec<Expr>,
        assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        debug!(?instance, "Replace allocation");
        let loc = tcx.codegen_span_option(span);
        let target = target.unwrap();
        let size = fargs.remove(0);
        Stmt::block(
            vec![
                unwrap_or_return_codegen_unimplemented_stmt!(tcx, tcx.codegen_place(&assign_to))
                    .goto_expr
                    .assign(
                        BuiltinFn::Malloc
                            .call(vec![size], loc)
                            .cast_to(Type::unsigned_int(8).to_pointer()),
                        loc,
                    ),
                Stmt::goto(tcx.current_fn().find_label(&target), Location::none()),
            ],
            Location::none(),
        )
    }
}

struct SliceFromRawPart;

impl<'tcx> GotocHook<'tcx> for SliceFromRawPart {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        let name = with_no_trimmed_paths!(tcx.def_path_str(instance.def_id()));
        name == "core::ptr::slice_from_raw_parts"
            || name == "std::ptr::slice_from_raw_parts"
            || name == "core::ptr::slice_from_raw_parts_mut"
            || name == "std::ptr::slice_from_raw_parts_mut"
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        _instance: Instance<'tcx>,
        mut fargs: Vec<Expr>,
        assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        let loc = tcx.codegen_span_option(span);
        let target = target.unwrap();
        let pt = tcx.codegen_ty(tcx.place_ty(&assign_to));
        let data = fargs.remove(0);
        let len = fargs.remove(0);
        let code = unwrap_or_return_codegen_unimplemented_stmt!(tcx, tcx.codegen_place(&assign_to))
            .goto_expr
            .assign(Expr::struct_expr_from_values(pt, vec![data, len], &tcx.symbol_table), loc)
            .with_location(loc);
        Stmt::block(vec![code, Stmt::goto(tcx.current_fn().find_label(&target), loc)], loc)
    }
}

/// This hook intercepts calls to `memcmp` and skips CBMC's pointer checks if the number of bytes to be compared is zero.
/// See issue <https://github.com/model-checking/kani/issues/1489>
///
/// This compiles `memcmp(first, second, count)` to:
/// ```c
/// count_var = count;
/// first_var = first;
/// second_var = second;
/// count_var == 0 && first_var != NULL && second_var != NULL ? 0 : memcmp(first_var, second_var, count_var)
/// ```
pub struct MemCmp;

impl<'tcx> GotocHook<'tcx> for MemCmp {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        let name = with_no_trimmed_paths!(tcx.def_path_str(instance.def_id()));
        name == "core::slice::cmp::memcmp" || name == "std::slice::cmp::memcmp"
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        instance: Instance<'tcx>,
        mut fargs: Vec<Expr>,
        assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        let loc = tcx.codegen_span_option(span);
        let target = target.unwrap();
        let first = fargs.remove(0);
        let second = fargs.remove(0);
        let count = fargs.remove(0);
        let (count_var, count_decl) = tcx.decl_temp_variable(count.typ().clone(), Some(count), loc);
        let (first_var, first_decl) = tcx.decl_temp_variable(first.typ().clone(), Some(first), loc);
        let (second_var, second_decl) =
            tcx.decl_temp_variable(second.typ().clone(), Some(second), loc);
        let is_count_zero = count_var.clone().is_zero();
        // We have to ensure that the pointers are valid even if we're comparing zero bytes.
        // According to Rust's current definition (see https://github.com/model-checking/kani/issues/1489),
        // this means they have to be non-null and aligned.
        // But alignment is automatically satisfied because `memcmp` takes `*const u8` pointers.
        let is_first_ok = first_var.clone().is_nonnull();
        let is_second_ok = second_var.clone().is_nonnull();
        let should_skip_pointer_checks = is_count_zero.and(is_first_ok).and(is_second_ok);
        let place_expr =
            unwrap_or_return_codegen_unimplemented_stmt!(tcx, tcx.codegen_place(&assign_to))
                .goto_expr;
        let rhs = should_skip_pointer_checks.ternary(
            Expr::int_constant(0, place_expr.typ().clone()), // zero bytes are always equal (as long as pointers are nonnull and aligned)
            tcx.codegen_func_expr(instance, span.as_ref())
                .call(vec![first_var, second_var, count_var]),
        );
        let code = place_expr.assign(rhs, loc).with_location(loc);
        Stmt::block(
            vec![
                count_decl,
                first_decl,
                second_decl,
                code,
                Stmt::goto(tcx.current_fn().find_label(&target), loc),
            ],
            loc,
        )
    }
}

/// This hook intercepts calls to `std::ptr::align_offset<T>` as CBMC's memory model has no concept
/// of alignment of allocations, so we would have to non-deterministically choose an alignment of
/// the base pointer, add the pointer's offset to it, and then do the math that is done in
/// `library/core/src/ptr/mod.rs`. Instead, we choose to always return `usize::MAX`, per
/// `align_offset`'s documentation, which states: "It is permissible for the implementation to
/// always return usize::MAX. Only your algorithm’s performance can depend on getting a usable
/// offset here, not its correctness."
pub struct AlignOffset;

impl<'tcx> GotocHook<'tcx> for AlignOffset {
    fn hook_applies(&self, tcx: TyCtxt<'tcx>, instance: Instance<'tcx>) -> bool {
        let name = with_no_trimmed_paths!(tcx.def_path_str(instance.def_id()));
        name == "std::ptr::align_offset"
    }

    fn handle(
        &self,
        tcx: &mut GotocCtx<'tcx>,
        _instance: Instance<'tcx>,
        mut _fargs: Vec<Expr>,
        assign_to: Place<'tcx>,
        target: Option<BasicBlock>,
        span: Option<Span>,
    ) -> Stmt {
        let loc = tcx.codegen_span_option(span);
        let target = target.unwrap();
        let place_expr =
            unwrap_or_return_codegen_unimplemented_stmt!(tcx, tcx.codegen_place(&assign_to))
                .goto_expr;
        let rhs = Expr::int_constant(usize::MAX, place_expr.typ().clone());
        let code = place_expr.assign(rhs, loc).with_location(loc);
        Stmt::block(vec![code, Stmt::goto(tcx.current_fn().find_label(&target), loc)], loc)
    }
}

pub fn fn_hooks<'tcx>() -> GotocHooks<'tcx> {
    GotocHooks {
        hooks: vec![
            Rc::new(Panic),
            Rc::new(Assume),
            Rc::new(Assert),
            Rc::new(Cover),
            Rc::new(Nondet),
            Rc::new(RustAlloc),
            Rc::new(SliceFromRawPart),
            Rc::new(MemCmp),
            Rc::new(AlignOffset),
        ],
    }
}

pub struct GotocHooks<'tcx> {
    hooks: Vec<Rc<dyn GotocHook<'tcx> + 'tcx>>,
}

impl<'tcx> GotocHooks<'tcx> {
    pub fn hook_applies(
        &self,
        tcx: TyCtxt<'tcx>,
        instance: Instance<'tcx>,
    ) -> Option<Rc<dyn GotocHook<'tcx> + 'tcx>> {
        for h in &self.hooks {
            if h.hook_applies(tcx, instance) {
                return Some(h.clone());
            }
        }
        None
    }
}
