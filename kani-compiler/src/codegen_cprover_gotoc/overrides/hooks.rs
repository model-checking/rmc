// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module contains various codegen hooks for functions that require special handling.
//!
//! E.g.: Functions in the Kani library that generate assumptions or symbolic variables.
//!
//! It would be too nasty if we spread around these sort of undocumented hooks in place, so
//! this module addresses this issue.

use crate::codegen_cprover_gotoc::codegen::{PropertyClass, bb_label};
use crate::codegen_cprover_gotoc::{GotocCtx, utils};
use crate::kani_middle::attributes;
use crate::kani_middle::kani_functions::{KaniFunction, KaniHook};
use crate::unwrap_or_return_codegen_unimplemented_stmt;
use cbmc::goto_program::CIntType;
use cbmc::goto_program::Symbol as GotoSymbol;
use cbmc::goto_program::{BuiltinFn, Expr, Location, Stmt, Type};
use rustc_middle::ty::TyCtxt;
use rustc_smir::rustc_internal;
use stable_mir::mir::mono::Instance;
use stable_mir::mir::{BasicBlockIdx, Place};
use stable_mir::ty::ClosureKind;
use stable_mir::ty::RigidTy;
use stable_mir::{CrateDef, ty::Span};
use std::collections::HashMap;
use std::rc::Rc;
use tracing::debug;

use cbmc::goto_program::ExprValue;

pub trait GotocHook {
    /// if the hook applies, it means the codegen would do something special to it
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool;
    /// the handler for codegen
    fn handle(
        &self,
        gcx: &mut GotocCtx,
        instance: Instance,
        fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt;
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

const UNEXPECTED_CALL: &str = "Hooks from kani library handled as a map";

impl GotocHook for Cover {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        _assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let cond = fargs.remove(0).cast_to(Type::bool());
        let msg = fargs.remove(0);
        let msg = gcx.extract_const_message(&msg).unwrap();
        let target = target.unwrap();
        let caller_loc = gcx.codegen_caller_span_stable(span);

        let (msg, reach_stmt) = gcx.codegen_reachability_check(msg, span);

        Stmt::block(
            vec![
                reach_stmt,
                gcx.codegen_cover(cond, &msg, span),
                Stmt::goto(bb_label(target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct Assume;
impl GotocHook for Assume {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        _assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 1);
        let cond = fargs.remove(0).cast_to(Type::bool());
        let target = target.unwrap();
        let loc = gcx.codegen_span_stable(span);

        Stmt::block(vec![gcx.codegen_assume(cond, loc), Stmt::goto(bb_label(target), loc)], loc)
    }
}

struct Assert;
impl GotocHook for Assert {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        _assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let cond = fargs.remove(0).cast_to(Type::bool());
        let msg = fargs.remove(0);
        let msg = gcx.extract_const_message(&msg).unwrap();
        let target = target.unwrap();
        let caller_loc = gcx.codegen_caller_span_stable(span);

        let (msg, reach_stmt) = gcx.codegen_reachability_check(msg, span);

        Stmt::block(
            vec![
                reach_stmt,
                gcx.codegen_assert_assume(cond, PropertyClass::Assertion, &msg, caller_loc),
                Stmt::goto(bb_label(target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct UnsupportedCheck;
impl GotocHook for UnsupportedCheck {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        _assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let msg = fargs.pop().unwrap();
        let cond = fargs.pop().unwrap().cast_to(Type::bool());
        let msg = gcx.extract_const_message(&msg).unwrap();
        let target = target.unwrap();
        let caller_loc = gcx.codegen_caller_span_stable(span);
        Stmt::block(
            vec![
                gcx.codegen_assert_assume(
                    cond,
                    PropertyClass::UnsupportedConstruct,
                    &msg,
                    caller_loc,
                ),
                Stmt::goto(bb_label(target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct SafetyCheck;
impl GotocHook for SafetyCheck {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        _assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let msg = fargs.pop().unwrap();
        let cond = fargs.pop().unwrap().cast_to(Type::bool());
        let msg = gcx.extract_const_message(&msg).unwrap();
        let target = target.unwrap();
        let caller_loc = gcx.codegen_caller_span_stable(span);
        Stmt::block(
            vec![
                gcx.codegen_assert_assume(cond, PropertyClass::SafetyCheck, &msg, caller_loc),
                Stmt::goto(bb_label(target), caller_loc),
            ],
            caller_loc,
        )
    }
}

// TODO: Remove this and replace occurrences with `SanityCheck`.
struct Check;
impl GotocHook for Check {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        _assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let cond = fargs.remove(0).cast_to(Type::bool());
        let msg = fargs.remove(0);
        let msg = gcx.extract_const_message(&msg).unwrap();
        let target = target.unwrap();
        let caller_loc = gcx.codegen_caller_span_stable(span);

        let (msg, reach_stmt) = gcx.codegen_reachability_check(msg, span);

        Stmt::block(
            vec![
                reach_stmt,
                gcx.codegen_assert(cond, PropertyClass::Assertion, &msg, caller_loc),
                Stmt::goto(bb_label(target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct Nondet;

impl GotocHook for Nondet {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert!(fargs.is_empty());
        let loc = gcx.codegen_span_stable(span);
        let target = target.unwrap();
        let pt = gcx.place_ty_stable(assign_to);
        if pt.kind().is_unit() {
            Stmt::goto(bb_label(target), loc)
        } else {
            let pe = unwrap_or_return_codegen_unimplemented_stmt!(
                gcx,
                gcx.codegen_place_stable(assign_to, loc)
            )
            .goto_expr;
            Stmt::block(
                vec![
                    pe.assign(gcx.codegen_ty_stable(pt).nondet(), loc),
                    Stmt::goto(bb_label(target), loc),
                ],
                loc,
            )
        }
    }
}

struct Panic;

impl GotocHook for Panic {
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        let def_id = rustc_internal::internal(tcx, instance.def.def_id());
        Some(def_id) == tcx.lang_items().panic_fn()
            || tcx.has_attr(def_id, rustc_span::sym::rustc_const_panic_str)
            || Some(def_id) == tcx.lang_items().panic_fmt()
            || Some(def_id) == tcx.lang_items().begin_panic_fn()
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        fargs: Vec<Expr>,
        _assign_to: &Place,
        _target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        gcx.codegen_panic(span, fargs)
    }
}

/// Encodes __CPROVER_r_ok(ptr, size)
struct IsAllocated;
impl GotocHook for IsAllocated {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 2);
        let size = fargs.pop().unwrap();
        let ptr = fargs.pop().unwrap().cast_to(Type::void_pointer());
        let target = target.unwrap();
        let loc = gcx.codegen_caller_span_stable(span);
        let ret_place = unwrap_or_return_codegen_unimplemented_stmt!(
            gcx,
            gcx.codegen_place_stable(assign_to, loc)
        );
        let ret_type = ret_place.goto_expr.typ().clone();

        Stmt::block(
            vec![
                ret_place.goto_expr.assign(Expr::read_ok(ptr, size).cast_to(ret_type), loc),
                Stmt::goto(bb_label(target), loc),
            ],
            loc,
        )
    }
}

/// This is the hook for the `kani::float::float_to_int_in_range` intrinsic
/// TODO: This should be replaced by a Rust function instead so that it's
/// independent of the backend
struct FloatToIntInRange;
impl GotocHook for FloatToIntInRange {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        instance: Instance,
        mut fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 1);
        let float = fargs.remove(0);
        let target = target.unwrap();
        let loc = gcx.codegen_span_stable(span);

        let generic_args = instance.args().0;
        let RigidTy::Float(float_ty) = *generic_args[0].expect_ty().kind().rigid().unwrap() else {
            unreachable!()
        };
        let integral_ty = generic_args[1].expect_ty().kind().rigid().unwrap().clone();

        let is_in_range = utils::codegen_in_range_expr(
            &float,
            float_ty,
            integral_ty,
            gcx.symbol_table.machine_model(),
        )
        .cast_to(Type::CInteger(CIntType::Bool));

        let pe = unwrap_or_return_codegen_unimplemented_stmt!(
            gcx,
            gcx.codegen_place_stable(assign_to, loc)
        )
        .goto_expr;

        Stmt::block(vec![pe.assign(is_in_range, loc), Stmt::goto(bb_label(target), loc)], loc)
    }
}

/// Encodes __CPROVER_pointer_object(ptr)
struct PointerObject;
impl GotocHook for PointerObject {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 1);
        let ptr = fargs.pop().unwrap().cast_to(Type::void_pointer());
        let target = target.unwrap();
        let loc = gcx.codegen_caller_span_stable(span);
        let ret_place = unwrap_or_return_codegen_unimplemented_stmt!(
            gcx,
            gcx.codegen_place_stable(assign_to, loc)
        );
        let ret_type = ret_place.goto_expr.typ().clone();

        Stmt::block(
            vec![
                ret_place.goto_expr.assign(Expr::pointer_object(ptr).cast_to(ret_type), loc),
                Stmt::goto(bb_label(target), loc),
            ],
            loc,
        )
    }
}

/// Encodes __CPROVER_pointer_offset(ptr)
struct PointerOffset;
impl GotocHook for PointerOffset {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 1);
        let ptr = fargs.pop().unwrap().cast_to(Type::void_pointer());
        let target = target.unwrap();
        let loc = gcx.codegen_caller_span_stable(span);
        let ret_place = unwrap_or_return_codegen_unimplemented_stmt!(
            gcx,
            gcx.codegen_place_stable(assign_to, loc)
        );
        let ret_type = ret_place.goto_expr.typ().clone();

        Stmt::block(
            vec![
                ret_place.goto_expr.assign(Expr::pointer_offset(ptr).cast_to(ret_type), loc),
                Stmt::goto(bb_label(target), loc),
            ],
            loc,
        )
    }
}

struct RustAlloc;
// Removing this hook causes regression failures.
// https://github.com/model-checking/kani/issues/1170
impl GotocHook for RustAlloc {
    fn hook_applies(&self, _tcx: TyCtxt, instance: Instance) -> bool {
        let full_name = instance.name();
        full_name == "alloc::alloc::exchange_malloc"
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        instance: Instance,
        mut fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        debug!(?instance, "Replace allocation");
        let loc = gcx.codegen_span_stable(span);
        let target = target.unwrap();
        let size = fargs.remove(0);
        Stmt::block(
            vec![
                unwrap_or_return_codegen_unimplemented_stmt!(
                    gcx,
                    gcx.codegen_place_stable(assign_to, loc)
                )
                .goto_expr
                .assign(
                    BuiltinFn::Malloc
                        .call(vec![size], loc)
                        .cast_to(Type::unsigned_int(8).to_pointer()),
                    loc,
                ),
                Stmt::goto(bb_label(target), loc),
            ],
            loc,
        )
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

impl GotocHook for MemCmp {
    fn hook_applies(&self, _tcx: TyCtxt, instance: Instance) -> bool {
        let name = instance.name();
        name == "core::slice::cmp::memcmp" || name == "std::slice::cmp::memcmp"
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        instance: Instance,
        mut fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        let loc = gcx.codegen_span_stable(span);
        let target = target.unwrap();
        let first = fargs.remove(0);
        let second = fargs.remove(0);
        let count = fargs.remove(0);
        let (count_var, count_decl) = gcx.decl_temp_variable(count.typ().clone(), Some(count), loc);
        let (first_var, first_decl) = gcx.decl_temp_variable(first.typ().clone(), Some(first), loc);
        let (second_var, second_decl) =
            gcx.decl_temp_variable(second.typ().clone(), Some(second), loc);
        let is_count_zero = count_var.clone().is_zero();
        // We have to ensure that the pointers are valid even if we're comparing zero bytes.
        // According to Rust's current definition (see https://github.com/model-checking/kani/issues/1489),
        // this means they have to be non-null and aligned.
        // But alignment is automatically satisfied because `memcmp` takes `*const u8` pointers.
        let is_first_ok = first_var.clone().is_nonnull();
        let is_second_ok = second_var.clone().is_nonnull();
        let should_skip_pointer_checks = is_count_zero.and(is_first_ok).and(is_second_ok);
        let place_expr = unwrap_or_return_codegen_unimplemented_stmt!(
            gcx,
            gcx.codegen_place_stable(assign_to, loc)
        )
        .goto_expr;
        let rhs = should_skip_pointer_checks.ternary(
            Expr::int_constant(0, place_expr.typ().clone()), // zero bytes are always equal (as long as pointers are nonnull and aligned)
            gcx.codegen_func_expr(instance, loc).call(vec![first_var, second_var, count_var]),
        );
        let code = place_expr.assign(rhs, loc).with_location(loc);
        Stmt::block(
            vec![count_decl, first_decl, second_decl, code, Stmt::goto(bb_label(target), loc)],
            loc,
        )
    }
}

/// A builtin that is essentially a C-style dereference operation, creating an
/// unsafe shallow copy. Importantly either this copy or the original needs to
/// be `mem::forget`en or a double-free will occur.
///
/// Takes in a `&T` reference and returns a `T` (like clone would but without
/// cloning). Breaks ownership rules and is only used in the context of function
/// contracts where we can structurally guarantee the use is safe.
struct UntrackedDeref;

impl GotocHook for UntrackedDeref {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        mut fargs: Vec<Expr>,
        assign_to: &Place,
        _target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(
            fargs.len(),
            1,
            "Invariant broken. `untracked_deref` should only be given one argument. \
            This function should only be called from code generated by kani macros, \
            as such this is likely a code-generation error."
        );
        let loc = gcx.codegen_span_stable(span);
        Stmt::block(
            vec![Stmt::assign(
                unwrap_or_return_codegen_unimplemented_stmt!(
                    gcx,
                    gcx.codegen_place_stable(assign_to, loc)
                )
                .goto_expr,
                fargs.pop().unwrap().dereference(),
                loc,
            )],
            loc,
        )
    }
}

struct InitContracts;

/// CBMC contracts currently has a limitation where `free` has to be in scope.
/// However, if there is no dynamic allocation in the harness, slicing removes `free` from the
/// scope.
///
/// Thus, this function will basically translate into:
/// ```c
/// // This is a no-op.
/// free(NULL);
/// ```
impl GotocHook for InitContracts {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        _instance: Instance,
        fargs: Vec<Expr>,
        _assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        assert_eq!(fargs.len(), 0,);
        let loc = gcx.codegen_span_stable(span);
        Stmt::block(
            vec![
                BuiltinFn::Free
                    .call(vec![Expr::pointer_constant(0, Type::void_pointer())], loc)
                    .as_stmt(loc),
                Stmt::goto(bb_label(target.unwrap()), loc),
            ],
            loc,
        )
    }
}

/// A loop contract register function call is assumed to be
/// 1. of form `kani_register_loop_contract(inv)` where `inv`
///    is the closure wrapping loop invariants
/// 2. is the last statement in some loop, so that its `target`` is
///    the head of the loop
///
/// Such call will be translate to
/// ```c
/// goto target
/// ```
/// with loop invariants (call to the register function) annotated as
/// a named sub of the `goto`.
pub struct LoopInvariantRegister;

impl GotocHook for LoopInvariantRegister {
    fn hook_applies(&self, _tcx: TyCtxt, instance: Instance) -> bool {
        attributes::fn_marker(instance.def)
            .is_some_and(|marker| marker == "kani_register_loop_contract")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        instance: Instance,
        fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        let loc = gcx.codegen_span_stable(span);
        let func_exp = gcx.codegen_func_expr(instance, loc);

        gcx.has_loop_contracts = true;

        if gcx.queries.args().unstable_features.contains(&"loop-contracts".to_string()) {
            // When loop-contracts is enabled, codegen
            // free(0)
            // goto target --- with loop contracts annotated.

            // Add `free(0)` to make sure the body of `free` won't be dropped to
            // satisfy the requirement of DFCC.
            Stmt::block(
                vec![
                    BuiltinFn::Free
                        .call(vec![Expr::pointer_constant(0, Type::void_pointer())], loc)
                        .as_stmt(loc),
                    Stmt::goto(bb_label(target.unwrap()), loc).with_loop_contracts(
                        func_exp.call(fargs).cast_to(Type::CInteger(CIntType::Bool)),
                    ),
                ],
                loc,
            )
        } else {
            // When loop-contracts is not enabled, codegen
            // assign_to = true
            // goto target
            Stmt::block(
                vec![
                    unwrap_or_return_codegen_unimplemented_stmt!(
                        gcx,
                        gcx.codegen_place_stable(assign_to, loc)
                    )
                    .goto_expr
                    .assign(Expr::c_true(), loc),
                    Stmt::goto(bb_label(target.unwrap()), loc).with_loop_contracts(
                        func_exp.call(fargs).cast_to(Type::CInteger(CIntType::Bool)),
                    ),
                ],
                loc,
            )
        }
    }
}

struct Forall;
struct Exists;

#[derive(Debug, Clone, Copy)]
enum QuantifierKind {
    ForAll,
    Exists,
}

impl GotocHook for Forall {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        instance: Instance,
        fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        handle_quantifier(gcx, instance, fargs, assign_to, target, span, QuantifierKind::ForAll)
    }
}

impl GotocHook for Exists {
    fn hook_applies(&self, _tcx: TyCtxt, _instance: Instance) -> bool {
        unreachable!("{UNEXPECTED_CALL}")
    }

    fn handle(
        &self,
        gcx: &mut GotocCtx,
        instance: Instance,
        fargs: Vec<Expr>,
        assign_to: &Place,
        target: Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        handle_quantifier(gcx, instance, fargs, assign_to, target, span, QuantifierKind::Exists)
    }
}

fn handle_quantifier(
    gcx: &mut GotocCtx,
    instance: Instance,
    fargs: Vec<Expr>,
    assign_to: &Place,
    target: Option<BasicBlockIdx>,
    span: Span,
    quantifier_kind: QuantifierKind,
) -> Stmt {
    let loc = gcx.codegen_span_stable(span);
    let target = target.unwrap();
    let lower_bound = &fargs[0];
    let upper_bound = &fargs[1];
    let predicate = &fargs[2];

    let closure_call_expr = find_closure_call_expr(&instance, gcx, loc)
        .unwrap_or_else(|| unreachable!("Failed to find closure call expression"));

    let new_variable_expr = if let ExprValue::Symbol { identifier } = lower_bound.value() {
        let new_identifier = format!("{}_kani", identifier);
        let new_symbol = GotoSymbol::variable(
            new_identifier.clone(),
            new_identifier.clone(),
            lower_bound.typ().clone(),
            loc,
        );
        gcx.symbol_table.insert(new_symbol.clone());
        new_symbol.to_expr()
    } else {
        unreachable!("Variable is not a symbol");
    };

    let lower_bound_comparison = lower_bound.clone().le(new_variable_expr.clone());
    let upper_bound_comparison = new_variable_expr.clone().lt(upper_bound.clone());
    let new_range = lower_bound_comparison.and(upper_bound_comparison);

    let new_predicate = closure_call_expr
        .call(vec![Expr::address_of(predicate.clone()), new_variable_expr.clone()]);
    let domain = new_range.implies(new_predicate.clone());

    let quantifier_expr = match quantifier_kind {
        QuantifierKind::ForAll => Expr::forall_expr(Type::Bool, new_variable_expr, domain),
        QuantifierKind::Exists => Expr::exists_expr(Type::Bool, new_variable_expr, domain),
    };

    Stmt::block(
        vec![
            unwrap_or_return_codegen_unimplemented_stmt!(
                gcx,
                gcx.codegen_place_stable(assign_to, loc)
            )
            .goto_expr
            .assign(quantifier_expr.cast_to(Type::CInteger(CIntType::Bool)), loc),
            Stmt::goto(bb_label(target), loc),
        ],
        loc,
    )
}

fn find_closure_call_expr(instance: &Instance, gcx: &mut GotocCtx, loc: Location) -> Option<Expr> {
    for arg in instance.args().0.iter() {
        let arg_ty = arg.ty()?;
        let kind = arg_ty.kind();
        let arg_kind = kind.rigid()?;

        if let RigidTy::Closure(def_id, args) = arg_kind {
            let instance_closure =
                Instance::resolve_closure(*def_id, args, ClosureKind::Fn).ok()?;
            return Some(gcx.codegen_func_expr(instance_closure, loc));
        }
    }
    None
}

pub fn fn_hooks() -> GotocHooks {
    let kani_lib_hooks = [
        (KaniHook::Assert, Rc::new(Assert) as Rc<dyn GotocHook>),
        (KaniHook::Assume, Rc::new(Assume)),
        (KaniHook::Exists, Rc::new(Exists)),
        (KaniHook::Forall, Rc::new(Forall)),
        (KaniHook::Panic, Rc::new(Panic)),
        (KaniHook::Check, Rc::new(Check)),
        (KaniHook::Cover, Rc::new(Cover)),
        (KaniHook::AnyRaw, Rc::new(Nondet)),
        (KaniHook::SafetyCheck, Rc::new(SafetyCheck)),
        (KaniHook::IsAllocated, Rc::new(IsAllocated)),
        (KaniHook::PointerObject, Rc::new(PointerObject)),
        (KaniHook::PointerOffset, Rc::new(PointerOffset)),
        (KaniHook::UnsupportedCheck, Rc::new(UnsupportedCheck)),
        (KaniHook::UntrackedDeref, Rc::new(UntrackedDeref)),
        (KaniHook::InitContracts, Rc::new(InitContracts)),
        (KaniHook::FloatToIntInRange, Rc::new(FloatToIntInRange)),
    ];
    GotocHooks {
        kani_lib_hooks: HashMap::from(kani_lib_hooks),
        other_hooks: vec![
            Rc::new(Panic),
            Rc::new(RustAlloc),
            Rc::new(MemCmp),
            Rc::new(LoopInvariantRegister),
        ],
    }
}

pub struct GotocHooks {
    /// Match functions that are unique and defined in the Kani library, which we can prefetch
    /// using `KaniFunctions`.
    kani_lib_hooks: HashMap<KaniHook, Rc<dyn GotocHook>>,
    /// Match functions that are not defined in the Kani library, which we cannot prefetch
    /// beforehand.
    other_hooks: Vec<Rc<dyn GotocHook>>,
}

impl GotocHooks {
    pub fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> Option<Rc<dyn GotocHook>> {
        if let Ok(KaniFunction::Hook(hook)) = KaniFunction::try_from(instance) {
            Some(self.kani_lib_hooks[&hook].clone())
        } else {
            for h in &self.other_hooks {
                if h.hook_applies(tcx, instance) {
                    return Some(h.clone());
                }
            }
            None
        }
    }
}
