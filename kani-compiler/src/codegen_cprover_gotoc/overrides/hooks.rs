// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module contains various codegen hooks for functions that require special handling.
//!
//! E.g.: Functions in the Kani library that generate assumptions or symbolic variables.
//!
//! It would be too nasty if we spread around these sort of undocumented hooks in place, so
//! this module addresses this issue.

use crate::codegen_cprover_gotoc::GotocCtx;
use crate::codegen_cprover_gotoc::codegen::{PropertyClass, bb_label};
use crate::kani_middle::attributes::KaniAttributes;
use crate::kani_middle::attributes::matches_diagnostic as matches_function;
use crate::unwrap_or_return_codegen_unimplemented_stmt;
use cbmc::goto_program::CIntType;
use cbmc::goto_program::{BuiltinFn, Expr, Stmt, Type};
use rustc_middle::ty::TyCtxt;
use rustc_smir::rustc_internal;
use rustc_span::Symbol;
use stable_mir::mir::mono::Instance;
use stable_mir::mir::{BasicBlockIdx, Place};
use stable_mir::{CrateDef, ty::Span};
use std::rc::Rc;
use tracing::debug;

pub trait GotocHook {
    /// if the hook applies, it means the codegen would do something special to it
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool;
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
impl GotocHook for Cover {
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniCover")
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

/// A hook for Kani's `contract_cover` function.
/// This is only used internally for contract instrumentation.
/// We use it to check that a contract's preconditions are satisfiable and that its postconditions are reachable.
/// Unlike the standard `cover`, failing this check does cause verification failure.
struct ContractCover;
impl GotocHook for ContractCover {
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniContractCover")
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
                gcx.codegen_contract_cover(cond, &msg, span),
                Stmt::goto(bb_label(target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct Assume;
impl GotocHook for Assume {
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniAssume")
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
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniAssert")
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

        // Since `cond` might have side effects, assign it to a temporary
        // variable so that it's evaluated once, then assert and assume it
        // TODO: I don't think `cond` can have side effects, this is MIR, it's going to be temps
        let (tmp, decl) = gcx.decl_temp_variable(cond.typ().clone(), Some(cond), caller_loc);
        Stmt::block(
            vec![
                reach_stmt,
                decl,
                gcx.codegen_assert_assume(tmp, PropertyClass::Assertion, &msg, caller_loc),
                Stmt::goto(bb_label(target), caller_loc),
            ],
            caller_loc,
        )
    }
}

struct Check;
impl GotocHook for Check {
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniCheck")
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
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniAnyRaw")
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
            || matches_function(tcx, instance.def, "KaniPanic")
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
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniIsAllocated")
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

/// Encodes __CPROVER_pointer_object(ptr)
struct PointerObject;
impl GotocHook for PointerObject {
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniPointerObject")
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
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniPointerOffset")
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
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniUntrackedDeref")
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
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        matches_function(tcx, instance.def, "KaniInitContracts")
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
    fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> bool {
        KaniAttributes::for_instance(tcx, instance).fn_marker()
            == Some(Symbol::intern("kani_register_loop_contract"))
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

pub fn fn_hooks() -> GotocHooks {
    GotocHooks {
        hooks: vec![
            Rc::new(Panic),
            Rc::new(Assume),
            Rc::new(ContractCover),
            Rc::new(Assert),
            Rc::new(Check),
            Rc::new(Cover),
            Rc::new(Nondet),
            Rc::new(IsAllocated),
            Rc::new(PointerObject),
            Rc::new(PointerOffset),
            Rc::new(RustAlloc),
            Rc::new(MemCmp),
            Rc::new(UntrackedDeref),
            Rc::new(InitContracts),
            Rc::new(LoopInvariantRegister),
        ],
    }
}

pub struct GotocHooks {
    hooks: Vec<Rc<dyn GotocHook>>,
}

impl GotocHooks {
    pub fn hook_applies(&self, tcx: TyCtxt, instance: Instance) -> Option<Rc<dyn GotocHook>> {
        for h in &self.hooks {
            if h.hook_applies(tcx, instance) {
                return Some(h.clone());
            }
        }
        None
    }
}
