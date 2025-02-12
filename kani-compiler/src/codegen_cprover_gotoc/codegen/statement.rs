// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
use super::typ::FN_RETURN_VOID_VAR_NAME;
use super::typ::TypeExt;
use super::{PropertyClass, bb_label};
use crate::codegen_cprover_gotoc::codegen::function::rustc_smir::region_from_coverage_opaque;
use crate::codegen_cprover_gotoc::{GotocCtx, VtableCtx};
use crate::unwrap_or_return_codegen_unimplemented_stmt;
use cbmc::goto_program::{Expr, Location, Stmt, Type};
use rustc_abi::{FieldsShape, Primitive, TagEncoding, Variants};
use rustc_middle::ty::layout::LayoutOf;
use rustc_middle::ty::{List, TypingEnv};
use rustc_smir::rustc_internal;
use stable_mir::abi::{ArgAbi, FnAbi, PassMode};
use stable_mir::mir::mono::{Instance, InstanceKind};
use stable_mir::mir::{
    AssertMessage, BasicBlockIdx, CopyNonOverlapping, NonDivergingIntrinsic, Operand, Place,
    RETURN_LOCAL, Statement, StatementKind, SwitchTargets, Terminator, TerminatorKind,
};
use stable_mir::ty::{Abi, RigidTy, Span, Ty, TyKind, VariantIdx};
use tracing::{debug, debug_span, trace};

impl GotocCtx<'_> {
    /// Generate Goto-C for MIR [Statement]s.
    /// This does not cover all possible "statements" because MIR distinguishes between ordinary
    /// statements and [Terminator]s, which can exclusively appear at the end of a basic block.
    ///
    /// See [GotocCtx::codegen_terminator] for those.
    pub fn codegen_statement(&mut self, stmt: &Statement) -> Stmt {
        let _trace_span = debug_span!("CodegenStatement", statement = ?stmt).entered();
        debug!(?stmt, kind=?stmt.kind, "handling_statement");
        let location = self.codegen_span_stable(stmt.span);
        match &stmt.kind {
            StatementKind::Assign(lhs, rhs) => {
                let lty = self.place_ty_stable(lhs);
                let rty = self.rvalue_ty_stable(rhs);
                // we ignore assignment for all zero size types
                if self.is_zst_stable(lty) {
                    Stmt::skip(location)
                } else if lty.kind().is_fn_ptr() && rty.kind().is_fn() && !rty.kind().is_fn_ptr() {
                    // implicit address of a function pointer, e.g.
                    // let fp: fn() -> i32 = foo;
                    // where the reference is implicit.
                    unwrap_or_return_codegen_unimplemented_stmt!(
                        self,
                        self.codegen_place_stable(lhs, location)
                    )
                    .goto_expr
                    .assign(self.codegen_rvalue_stable(rhs, location).address_of(), location)
                } else if rty.kind().is_bool() {
                    unwrap_or_return_codegen_unimplemented_stmt!(
                        self,
                        self.codegen_place_stable(lhs, location)
                    )
                    .goto_expr
                    .assign(
                        self.codegen_rvalue_stable(rhs, location).cast_to(Type::c_bool()),
                        location,
                    )
                } else {
                    unwrap_or_return_codegen_unimplemented_stmt!(
                        self,
                        self.codegen_place_stable(lhs, location)
                    )
                    .goto_expr
                    .assign(self.codegen_rvalue_stable(rhs, location), location)
                }
            }
            StatementKind::Deinit(place) => self.codegen_deinit(place, location),
            StatementKind::SetDiscriminant { place, variant_index } => {
                let dest_ty = self.place_ty_stable(place);
                let dest_expr = unwrap_or_return_codegen_unimplemented_stmt!(
                    self,
                    self.codegen_place_stable(place, location)
                )
                .goto_expr;
                self.codegen_set_discriminant(dest_ty, dest_expr, *variant_index, location)
            }
            // StorageLive and StorageDead are modelled via CBMC's internal means of detecting
            // accesses to dangling pointers, which uses demonic non-determinism. That is, CBMC
            // non-deterministically chooses a single object's address to be tracked in a
            // pointer-typed global instrumentation variable __CPROVER_dead_object. Any dereference
            // entails a check that the pointer being dereferenced is not equal to the pointer held
            // in __CPROVER_dead_object. We use this to bridge the difference between Rust and MIR
            // semantics as follows:
            //
            // 1. (At the time of writing) MIR declares all function-local variables at function
            //    scope, irrespective of the scope/block that Rust code originally used.
            // 2. In MIR, StorageLive and StorageDead markers are inserted at the beginning and end
            //    of the Rust block to record the Rust-level lifetime of the object.
            // 3. We translate MIR declarations into GOTO declarations, implying that we will have
            //    a single object per function for a local variable, even when Rust had a variable
            //    declared in a sub-scope of the function where said scope was entered multiple
            //    times (e.g., a loop body).
            // 4. To enable detection of use of dangling pointers, we now use
            //    __CPROVER_dead_object, unless the address of the local object is never taken
            //    (implying that there cannot be a use of a dangling pointer with respect to said
            //    object). We update __CPROVER_dead_object as follows:
            //    * StorageLive is set to NULL when __CPROVER_dead_object pointed to the object
            //    (re-)entering scope, or else is left unchanged.
            //    * StorageDead non-deterministically updates (or leaves unchanged)
            //    __CPROVER_dead_object to point to the object going out of scope. (This is the
            //    same update approach as used within CBMC.)
            //
            // This approach will also work when there are multiple occurrences of StorageLive (or
            // StorageDead) on a path, or across control-flow branches, and even when StorageDead
            // occurs without a preceding StorageLive.
            StatementKind::StorageLive(var_id) => {
                if !self.current_fn().is_address_taken_local(*var_id) {
                    Stmt::skip(location)
                } else {
                    let global_dead_object = cbmc::global_dead_object(&self.symbol_table);
                    Stmt::assign(
                        global_dead_object.clone(),
                        global_dead_object
                            .clone()
                            .eq(self
                                .codegen_local(*var_id, location)
                                .address_of()
                                .cast_to(global_dead_object.typ().clone()))
                            .ternary(global_dead_object.typ().null(), global_dead_object),
                        location,
                    )
                }
            }
            StatementKind::StorageDead(var_id) => {
                if !self.current_fn().is_address_taken_local(*var_id) {
                    Stmt::skip(location)
                } else {
                    let global_dead_object = cbmc::global_dead_object(&self.symbol_table);
                    Stmt::assign(
                        global_dead_object.clone(),
                        Type::bool().nondet().ternary(
                            self.codegen_local(*var_id, location)
                                .address_of()
                                .cast_to(global_dead_object.typ().clone()),
                            global_dead_object,
                        ),
                        location,
                    )
                }
            }
            StatementKind::Intrinsic(NonDivergingIntrinsic::CopyNonOverlapping(
                CopyNonOverlapping { src, dst, count },
            )) => {
                let operands = [src, dst, count];
                // Pack the operands and their types, then call `codegen_copy`
                let fargs =
                    operands.iter().map(|op| self.codegen_operand_stable(op)).collect::<Vec<_>>();
                let farg_types = operands.map(|op| self.operand_ty_stable(&op));
                self.codegen_copy("copy_nonoverlapping", true, fargs, &farg_types, None, location)
            }
            StatementKind::Intrinsic(NonDivergingIntrinsic::Assume(ref op)) => {
                let cond = self.codegen_operand_stable(op).cast_to(Type::bool());
                self.codegen_assert_assume(
                    cond,
                    PropertyClass::Assume,
                    "assumption failed",
                    location,
                )
            }
            StatementKind::Coverage(coverage_opaque) => {
                let function_name = self.current_fn().readable_name();
                let instance = self.current_fn().instance_stable();
                let counter_data = format!("{coverage_opaque:?} ${function_name}$");
                let maybe_source_region =
                    region_from_coverage_opaque(self.tcx, &coverage_opaque, instance);
                if let Some((source_region, file_name)) = maybe_source_region {
                    let coverage_stmt =
                        self.codegen_coverage(&counter_data, stmt.span, source_region, &file_name);
                    // TODO: Avoid single-statement blocks when conversion of
                    // standalone statements to the irep format is fixed.
                    // More details in <https://github.com/model-checking/kani/issues/3012>
                    Stmt::block(vec![coverage_stmt], location)
                } else {
                    Stmt::skip(location)
                }
            }
            StatementKind::PlaceMention(_) => todo!(),
            StatementKind::FakeRead(..)
            | StatementKind::Retag(_, _)
            | StatementKind::AscribeUserType { .. }
            | StatementKind::Nop
            | StatementKind::ConstEvalCounter => Stmt::skip(location),
        }
        .with_location(location)
    }

    /// Generate Goto-c for MIR [Terminator] statements.
    /// Many kinds of seemingly ordinary statements in Rust are "terminators" (i.e. the sort of statement that _ends_ a basic block)
    /// because of the need for unwinding/drop. For instance, function calls.
    ///
    /// See also [`GotocCtx::codegen_statement`] for ordinary [Statement]s.
    pub fn codegen_terminator(&mut self, term: &Terminator) -> Stmt {
        let loc = self.codegen_span_stable(term.span);
        let _trace_span = debug_span!("CodegenTerminator", statement = ?term.kind).entered();
        debug!("handling terminator {:?}", term);
        //TODO: Instead of doing location::none(), and updating, just putit in when we make the stmt.
        match &term.kind {
            TerminatorKind::Goto { target } => Stmt::goto(bb_label(*target), loc),
            TerminatorKind::SwitchInt { discr, targets } => {
                self.codegen_switch_int(discr, targets, loc)
            }
            // The following two use `codegen_mimic_unimplemented`
            // because we don't want to raise the warning during compilation.
            // These operations will normally be codegen'd but normally be unreachable
            // since we make use of `-C unwind=abort`.
            TerminatorKind::Resume => self.codegen_mimic_unimplemented(
                "TerminatorKind::Resume",
                loc,
                "https://github.com/model-checking/kani/issues/692",
            ),
            TerminatorKind::Abort => self.codegen_mimic_unimplemented(
                "TerminatorKind::Abort",
                loc,
                "https://github.com/model-checking/kani/issues/692",
            ),
            TerminatorKind::Return => {
                let rty = self.current_fn().instance_stable().fn_abi().unwrap().ret.ty;
                if rty.kind().is_unit() {
                    self.codegen_ret_unit(loc)
                } else {
                    let place = Place::from(RETURN_LOCAL);
                    let place_expr = unwrap_or_return_codegen_unimplemented_stmt!(
                        self,
                        self.codegen_place_stable(&place, loc)
                    )
                    .goto_expr;
                    assert_eq!(rty, self.place_ty_stable(&place), "Unexpected return type");
                    if rty.kind().is_bool() {
                        place_expr.cast_to(Type::c_bool()).ret(loc)
                    } else {
                        place_expr.ret(loc)
                    }
                }
            }
            TerminatorKind::Unreachable => self.codegen_assert_assume_false(
                PropertyClass::Unreachable,
                "unreachable code",
                loc,
            ),
            TerminatorKind::Drop { place, target, unwind: _ } => {
                self.codegen_drop(place, target, loc)
            }
            TerminatorKind::Call { func, args, destination, target, .. } => {
                self.codegen_funcall(func, args, destination, target, term.span)
            }
            TerminatorKind::Assert { cond, expected, msg, target, .. } => {
                let cond = {
                    let r = self.codegen_operand_stable(cond);
                    if *expected { r } else { Expr::not(r) }
                };

                let (msg, property_class) = if let AssertMessage::BoundsCheck { .. } = msg {
                    // For bounds check the following panic message is generated at runtime:
                    // "index out of bounds: the length is {len} but the index is {index}",
                    // but CBMC only accepts static messages so we don't add values to the message.
                    (
                        "index out of bounds: the length is less than or equal to the given index",
                        PropertyClass::Assertion,
                    )
                } else if let AssertMessage::MisalignedPointerDereference { .. } = msg {
                    // Misaligned pointer dereference check messages is also a runtime messages.
                    // Generate a generic one here.
                    (
                        "misaligned pointer dereference: address must be a multiple of its type's \
                    alignment",
                        PropertyClass::SafetyCheck,
                    )
                } else {
                    // For all other assert kind we can get the static message.
                    (msg.description().unwrap(), PropertyClass::Assertion)
                };

                let (msg_str, reach_stmt) =
                    self.codegen_reachability_check(msg.to_owned(), term.span);

                Stmt::block(
                    vec![
                        reach_stmt,
                        self.codegen_assert_assume(
                            cond.cast_to(Type::bool()),
                            property_class,
                            &msg_str,
                            loc,
                        ),
                        Stmt::goto(bb_label(*target), loc),
                    ],
                    loc,
                )
            }
            TerminatorKind::InlineAsm { .. } => self.codegen_unimplemented_stmt(
                "TerminatorKind::InlineAsm",
                loc,
                "https://github.com/model-checking/kani/issues/2",
            ),
        }
    }

    /// Create a statement that sets the variable discriminant to the value that corresponds to the
    /// variant index.
    pub fn codegen_set_discriminant(
        &mut self,
        dest_ty: Ty,
        dest_expr: Expr,
        variant_index: VariantIdx,
        location: Location,
    ) -> Stmt {
        // this requires place points to an enum type.
        let dest_ty_internal = rustc_internal::internal(self.tcx, dest_ty);
        let variant_index_internal = rustc_internal::internal(self.tcx, variant_index);
        let layout = self.layout_of(dest_ty_internal);
        match &layout.variants {
            Variants::Empty | Variants::Single { .. } => Stmt::skip(location),
            Variants::Multiple { tag, tag_encoding, .. } => match tag_encoding {
                TagEncoding::Direct => {
                    let discr = dest_ty_internal
                        .discriminant_for_variant(self.tcx, variant_index_internal)
                        .unwrap();
                    let discr_t = self.codegen_enum_discr_typ(dest_ty_internal);
                    // The constant created below may not fit into the type.
                    // https://github.com/model-checking/kani/issues/996
                    //
                    // It doesn't matter if the type comes from `self.codegen_enum_discr_typ(pt)`
                    // or `discr.ty`. It looks like something is wrong with `discriminat_for_variant`
                    // because when it tries to codegen `std::cmp::Ordering` (which should produce
                    // discriminant values -1, 0 and 1) it produces values 255, 0 and 1 with i8 types:
                    //
                    // debug!("DISCRIMINANT - val:{:?} ty:{:?}", discr.val, discr.ty);
                    // DISCRIMINANT - val:255 ty:i8
                    // DISCRIMINANT - val:0 ty:i8
                    // DISCRIMINANT - val:1 ty:i8
                    trace!(?discr, ?discr_t, ?dest_ty, "codegen_set_discriminant direct");
                    // The discr.ty doesn't always match the tag type. Explicitly cast if needed.
                    let discr_expr = Expr::int_constant(discr.val, self.codegen_ty(discr.ty))
                        .cast_to(self.codegen_ty(discr_t));
                    self.codegen_discriminant_field(dest_expr, dest_ty).assign(discr_expr, location)
                }
                TagEncoding::Niche { untagged_variant, niche_variants, niche_start } => {
                    if *untagged_variant != variant_index_internal {
                        let offset = match &layout.fields {
                            FieldsShape::Arbitrary { offsets, .. } => offsets[0usize.into()],
                            _ => unreachable!("niche encoding must have arbitrary fields"),
                        };
                        let discr_ty = self.codegen_enum_discr_typ(dest_ty_internal);
                        let discr_ty = self.codegen_ty(discr_ty);
                        let niche_value =
                            variant_index_internal.as_u32() - niche_variants.start().as_u32();
                        let niche_value = (niche_value as u128).wrapping_add(*niche_start);
                        trace!(val=?niche_value, typ=?discr_ty, "codegen_set_discriminant niche");
                        let value = if niche_value == 0
                            && matches!(tag.primitive(), Primitive::Pointer(_))
                        {
                            discr_ty.null()
                        } else {
                            Expr::int_constant(niche_value, discr_ty.clone())
                        };
                        self.codegen_get_niche(dest_expr, offset.bytes() as usize, discr_ty)
                            .assign(value, location)
                    } else {
                        Stmt::skip(location)
                    }
                }
            },
        }
    }

    /// From rustc doc: "This writes `uninit` bytes to the entire place."
    /// Our model of GotoC has a similar statement, which is later lowered
    /// to assigning a Nondet in CBMC, with a comment specifying that it
    /// corresponds to a Deinit.
    fn codegen_deinit(&mut self, place: &Place, loc: Location) -> Stmt {
        let dst_mir_ty = self.place_ty_stable(place);
        let dst_type = self.codegen_ty_stable(dst_mir_ty);
        let layout = self.layout_of_stable(dst_mir_ty);
        if layout.is_zst() || dst_type.sizeof_in_bits(&self.symbol_table) == 0 {
            // We ignore assignment for all zero size types
            Stmt::skip(loc)
        } else {
            unwrap_or_return_codegen_unimplemented_stmt!(
                self,
                self.codegen_place_stable(place, loc)
            )
            .goto_expr
            .deinit(loc)
        }
    }

    /// A special case handler to codegen `return ();`
    fn codegen_ret_unit(&mut self, loc: Location) -> Stmt {
        let is_file_local = false;
        let ty = self.codegen_ty_unit();
        let var = self.ensure_global_var(FN_RETURN_VOID_VAR_NAME, is_file_local, ty, loc);
        Stmt::ret(Some(var.to_expr()), loc)
    }

    /// Generates Goto-C for MIR [TerminatorKind::Drop] calls. We only handle code _after_ Rust's "drop elaboration"
    /// transformation, so these have a simpler semantics.
    ///
    /// The generated code should invoke the appropriate `drop` function on `place`, then goto `target`.
    ///
    /// TODO: this function doesn't handle unwinding which begins if the destructor panics
    /// <https://github.com/model-checking/kani/issues/221>
    fn codegen_drop(&mut self, place: &Place, target: &BasicBlockIdx, loc: Location) -> Stmt {
        let place_ty = self.place_ty_stable(place);
        let drop_instance = Instance::resolve_drop_in_place(place_ty);
        debug!(?place_ty, ?drop_instance, "codegen_drop");
        // Once upon a time we did a `hook_applies` check here, but we no longer seem to hook drops
        let drop_implementation = match drop_instance.kind {
            InstanceKind::Shim if drop_instance.is_empty_shim() => {
                // We can skip empty DropGlue functions
                Stmt::skip(loc)
            }
            InstanceKind::Shim => {
                // Since the reference is used right away here, no need to inject a check for pointer validity.
                let place_ref = self.codegen_place_ref_stable(place, loc);
                match place_ty.kind() {
                    TyKind::RigidTy(RigidTy::Dynamic(..)) => {
                        // Virtual drop via a vtable lookup.
                        // Pull the drop function off of the fat pointer's vtable pointer
                        let vtable_ref = place_ref.to_owned().member("vtable", &self.symbol_table);
                        let data_ref = place_ref.to_owned().member("data", &self.symbol_table);
                        let vtable = vtable_ref.dereference();
                        let fn_ptr = vtable.member("drop", &self.symbol_table);
                        trace!(?fn_ptr, ?data_ref, "codegen_drop");

                        let call = fn_ptr.dereference().call(vec![data_ref]).as_stmt(loc);
                        if self.vtable_ctx.emit_vtable_restrictions {
                            self.virtual_call_with_restricted_fn_ptr(
                                place_ref.typ().clone(),
                                VtableCtx::drop_index(),
                                call,
                            )
                        } else {
                            call
                        }
                    }
                    _ => {
                        // Non-virtual, direct drop_in_place call
                        assert!(!matches!(drop_instance.kind, InstanceKind::Virtual { .. }));

                        let func = self.codegen_func_expr(drop_instance, loc);
                        // The only argument should be a self reference
                        let args = vec![place_ref];

                        // We have a known issue where nested Arc and Mutex objects result in
                        // drop_in_place call implementations that fail to typecheck. Skipping
                        // drop entirely causes unsound verification results in common cases
                        // like vector extend, so for now, add a sound special case workaround
                        // for calls that fail the typecheck.
                        // https://github.com/model-checking/kani/issues/426
                        // Unblocks: https://github.com/model-checking/kani/issues/435
                        func.call(args).as_stmt(loc)
                    }
                }
            }
            kind => unreachable!(
                "Expected a `InstanceKind::Shim` for `TerminatorKind::Drop`, but found {kind:?}"
            ),
        };
        let goto_target = Stmt::goto(bb_label(*target), loc);
        let block = vec![drop_implementation, goto_target];
        Stmt::block(block, loc)
    }

    /// Generates Goto-C for MIR [TerminatorKind::SwitchInt].
    /// Operand evaluates to an integer;
    /// jump depending on its value to one of the targets, and otherwise fallback to `targets.otherwise()`.
    /// The otherwise value is stores as the last value of targets.
    fn codegen_switch_int(
        &mut self,
        discr: &Operand,
        targets: &SwitchTargets,
        loc: Location,
    ) -> Stmt {
        let v = self.codegen_operand_stable(discr);
        let switch_ty = v.typ().clone();

        // Switches with empty branches should've been eliminated already.
        assert!(targets.len() > 1);
        if targets.len() == 2 {
            // Translate to a guarded goto
            let (case, first_target) = targets.branches().next().unwrap();
            Stmt::block(
                vec![
                    v.eq(Expr::int_constant(case, switch_ty)).if_then_else(
                        Stmt::goto(bb_label(first_target), loc),
                        None,
                        loc,
                    ),
                    Stmt::goto(bb_label(targets.otherwise()), loc),
                ],
                loc,
            )
        } else {
            let cases = targets
                .branches()
                .map(|(c, bb)| {
                    Expr::int_constant(c, switch_ty.clone())
                        .with_location(loc)
                        .switch_case(Stmt::goto(bb_label(bb), loc))
                })
                .collect();
            let default = Stmt::goto(bb_label(targets.otherwise()), loc);
            v.switch(cases, Some(default), loc)
        }
    }

    /// As part of **calling** a function (or closure), we may need to un-tuple arguments.
    ///
    /// This function will replace the last `fargs` argument by its un-tupled version.
    ///
    /// Some context: A closure / shim takes two arguments:
    ///     0. a struct (or a pointer to) representing the environment
    ///     1. a tuple containing the parameters (if not empty)
    ///
    /// However, Rust generates a function where the tuple of parameters are flattened
    /// as subsequent parameters.
    ///
    /// See [GotocCtx::ty_needs_untupled_args] for more details.
    fn codegen_untupled_args(&mut self, op: &Operand, args_abi: &[ArgAbi]) -> Vec<Expr> {
        let tuple_ty = self.operand_ty_stable(op);
        let tuple_expr = self.codegen_operand_stable(op);
        let TyKind::RigidTy(RigidTy::Tuple(tupled_args)) = tuple_ty.kind() else { unreachable!() };
        tupled_args
            .iter()
            .enumerate()
            .filter_map(|(idx, _)| {
                let arg_abi = &args_abi[idx];
                (arg_abi.mode != PassMode::Ignore).then(|| {
                    // Access the tupled parameters through the `member` operation
                    tuple_expr.clone().member(idx.to_string(), &self.symbol_table)
                })
            })
            .collect()
    }

    /// Because function calls terminate basic blocks, to "end" a function call, we
    /// must jump to the next basic block.
    fn codegen_end_call(&self, target: Option<BasicBlockIdx>, loc: Location) -> Stmt {
        if let Some(next_bb) = target {
            Stmt::goto(bb_label(next_bb), loc)
        } else {
            self.codegen_sanity(Expr::bool_false(), "Unexpected return from Never function", loc)
        }
    }

    /// Generate Goto-C for each argument to a function call.
    ///
    /// N.B. public only because instrinsics use this directly, too.
    pub(crate) fn codegen_funcall_args(&mut self, fn_abi: &FnAbi, args: &[Operand]) -> Vec<Expr> {
        let fargs: Vec<Expr> = args
            .iter()
            .enumerate()
            .filter_map(|(i, op)| {
                // Functions that require caller info will have an extra parameter.
                let arg_abi = &fn_abi.args.get(i);
                let ty = self.operand_ty_stable(op);
                if ty.kind().is_bool() {
                    Some(self.codegen_operand_stable(op).cast_to(Type::c_bool()))
                } else if arg_abi.is_none_or(|abi| abi.mode != PassMode::Ignore) {
                    Some(self.codegen_operand_stable(op))
                } else {
                    None
                }
            })
            .collect();
        debug!(?fargs, args_abi=?fn_abi.args, "codegen_funcall_args");
        fargs
    }

    /// Generates Goto-C for a MIR [TerminatorKind::Call] statement.
    ///
    /// This calls either:
    ///
    /// 1. A statically-known function definition.
    /// 2. A statically-known trait function, which gets a pointer out of a vtable.
    /// 2. A direct function pointer.
    ///
    /// Kani also performs a few alterations:
    ///
    /// 1. Do nothing for "empty drop glue"
    /// 2. If a Kani hook applies, do that instead.
    fn codegen_funcall(
        &mut self,
        func: &Operand,
        args: &[Operand],
        destination: &Place,
        target: &Option<BasicBlockIdx>,
        span: Span,
    ) -> Stmt {
        debug!(?func, ?args, ?destination, ?span, "codegen_funcall");
        let instance_opt = self.get_instance(func);
        if let Some(instance) = instance_opt
            && matches!(instance.kind, InstanceKind::Intrinsic)
        {
            let TyKind::RigidTy(RigidTy::FnDef(def, _)) = instance.ty().kind() else {
                unreachable!("Expected function type for intrinsic: {instance:?}")
            };
            // The compiler is currently transitioning how to handle intrinsic fallback body.
            // Until https://github.com/rust-lang/project-stable-mir/issues/79 is implemented
            // we have to check `must_be_overridden` and `has_body`.
            if def.as_intrinsic().unwrap().must_be_overridden() || !instance.has_body() {
                return self.codegen_funcall_of_intrinsic(
                    instance,
                    &args,
                    &destination,
                    target.map(|bb| bb),
                    span,
                );
            }
        }

        let loc = self.codegen_span_stable(span);
        let fn_ty = self.operand_ty_stable(func);
        match fn_ty.kind() {
            fn_def @ TyKind::RigidTy(RigidTy::FnDef(..)) => {
                let instance = instance_opt.unwrap();
                let fn_abi = instance.fn_abi().unwrap();
                let mut fargs = if args.is_empty()
                    || fn_def.fn_sig().unwrap().value.abi != Abi::RustCall
                {
                    self.codegen_funcall_args(&fn_abi, &args)
                } else {
                    let (untupled, first_args) = args.split_last().unwrap();
                    let mut fargs = self.codegen_funcall_args(&fn_abi, &first_args);
                    fargs.append(
                        &mut self.codegen_untupled_args(untupled, &fn_abi.args[first_args.len()..]),
                    );
                    fargs
                };

                if let Some(hk) = self.hooks.hook_applies(self.tcx, instance) {
                    return hk.handle(self, instance, fargs, destination, *target, span);
                }

                let mut stmts: Vec<Stmt> = match instance.kind {
                    // Here an empty drop glue is invoked; we just ignore it.
                    InstanceKind::Shim if instance.is_empty_shim() => {
                        return Stmt::goto(bb_label(target.unwrap()), loc);
                    }
                    // Handle a virtual function call via a vtable lookup
                    InstanceKind::Virtual { idx } => {
                        let self_ty = self.operand_ty_stable(&args[0]);
                        self.codegen_virtual_funcall(self_ty, idx, destination, &mut fargs, loc)
                    }
                    // Normal, non-virtual function calls
                    InstanceKind::Item | InstanceKind::Intrinsic | InstanceKind::Shim => {
                        // We need to handle FnDef items in a special way because `codegen_operand` compiles them to dummy structs.
                        // (cf. the function documentation)
                        let func_exp = self.codegen_func_expr(instance, loc);
                        if instance.is_foreign_item() {
                            vec![self.codegen_foreign_call(func_exp, fargs, destination, loc)]
                        } else {
                            vec![self.codegen_expr_to_place_stable(
                                destination,
                                func_exp.call(fargs),
                                loc,
                            )]
                        }
                    }
                };
                stmts.push(self.codegen_end_call(*target, loc));
                Stmt::block(stmts, loc)
            }
            // Function call through a pointer
            TyKind::RigidTy(RigidTy::FnPtr(fn_sig)) => {
                let fn_sig_internal = rustc_internal::internal(self.tcx, fn_sig);
                let fn_ptr_abi = rustc_internal::stable(
                    self.tcx
                        .fn_abi_of_fn_ptr(
                            TypingEnv::fully_monomorphized()
                                .as_query_input((fn_sig_internal, &List::empty())),
                        )
                        .unwrap(),
                );
                let fargs = self.codegen_funcall_args(&fn_ptr_abi, &args);
                let func_expr = self.codegen_operand_stable(func).dereference();
                // Actually generate the function call and return.
                Stmt::block(
                    vec![
                        self.codegen_expr_to_place_stable(destination, func_expr.call(fargs), loc),
                        Stmt::goto(bb_label(target.unwrap()), loc),
                    ],
                    loc,
                )
            }
            x => unreachable!("Function call where the function was of unexpected type: {:?}", x),
        }
    }

    /// Extract a reference to self for virtual method calls.
    ///
    /// See [GotocCtx::codegen_dynamic_function_sig] for more details.
    fn extract_ptr(&self, arg_expr: Expr, arg_ty: Ty) -> Expr {
        // Generate an expression that indexes the pointer.
        let expr = self
            .receiver_data_path(rustc_internal::internal(self.tcx, arg_ty))
            .fold(arg_expr, |curr_expr, (name, _)| curr_expr.member(name, &self.symbol_table));

        trace!(?arg_ty, gotoc_ty=?expr.typ(), gotoc_expr=?expr.value(), "extract_ptr");
        expr
    }

    /// Codegen the dynamic call to a trait method via the fat pointer vtable.
    ///
    /// If the original call was of the form
    /// f(arg0, arg1);
    ///
    /// The new call should be of the form
    /// arg0.vtable->f(arg0.data,arg1);
    ///
    /// For that, we do the following:
    /// 1. Extract the fat pointer out of the first argument.
    /// 2. Obtain the function pointer out of the fat pointer vtable.
    /// 3. Change the first argument to only reference the data pointer (instead of the fat one).
    ///     - When the receiver type is a `struct` we need to build a structure that mirrors
    ///       the original one but uses a thin pointer instead.
    /// 4. Generate the function call.
    fn codegen_virtual_funcall(
        &mut self,
        self_ty: Ty,
        idx: usize,
        place: &Place,
        fargs: &mut [Expr],
        loc: Location,
    ) -> Vec<Stmt> {
        let vtable_field_name = self.vtable_field_name(idx);
        trace!(?self_ty, ?place, ?vtable_field_name, "codegen_virtual_funcall");
        debug!(?fargs, "codegen_virtual_funcall");

        let trait_fat_ptr = self.extract_ptr(fargs[0].clone(), self_ty);
        assert!(
            trait_fat_ptr.typ().is_rust_trait_fat_ptr(&self.symbol_table),
            "Expected fat pointer, but got {:?}",
            trait_fat_ptr.typ()
        );

        let vtable_ref = trait_fat_ptr.to_owned().member("vtable", &self.symbol_table);
        let vtable = vtable_ref.dereference();
        let fn_ptr = vtable.member(vtable_field_name, &self.symbol_table);
        trace!(fn_typ=?fn_ptr.typ(), "codegen_virtual_funcall");

        let data_ptr = trait_fat_ptr.to_owned().member("data", &self.symbol_table);
        let mut ret_stmts = vec![];
        fargs[0] = if self_ty.kind().is_adt() {
            // Generate a temp variable and assign its inner pointer to the fat_ptr.data.
            match fn_ptr.typ() {
                Type::Pointer { typ: box Type::Code { parameters, .. } } => {
                    let param_typ = parameters.first().unwrap().typ();
                    let (tmp, decl) = self.decl_temp_variable(param_typ.clone(), None, loc);
                    debug!(?tmp,
                        orig=?data_ptr.typ(),
                        "codegen_virtual_funcall");
                    ret_stmts.push(decl);
                    ret_stmts.push(Stmt::assign(
                        self.extract_ptr(tmp.clone(), self_ty),
                        data_ptr,
                        loc,
                    ));
                    tmp
                }
                _ => unreachable!("Unexpected virtual function type: {:?}", fn_ptr.typ()),
            }
        } else {
            // Update the argument from arg0 to arg0.data if arg0 is a fat pointer.
            data_ptr
        };

        // For soundness, add an assertion that the vtable function call is not null.
        // Otherwise, CBMC might treat this as an assume(0) and later user-added assertions
        // could become unreachable.
        let call_is_nonnull = fn_ptr.clone().is_nonnull();
        let assert_msg = format!("Non-null virtual function call for {vtable_field_name:?}");
        let assert_nonnull = self.codegen_sanity(call_is_nonnull, &assert_msg, loc);

        // Virtual function call and corresponding nonnull assertion.
        let call = fn_ptr.dereference().call(fargs.to_vec());
        let call_stmt = self.codegen_expr_to_place_stable(place, call, loc);
        let call_stmt = if self.vtable_ctx.emit_vtable_restrictions {
            self.virtual_call_with_restricted_fn_ptr(trait_fat_ptr.typ().clone(), idx, call_stmt)
        } else {
            call_stmt
        };
        ret_stmts.push(assert_nonnull);
        ret_stmts.push(call_stmt);
        ret_stmts
    }

    /// Generates Goto-C to assign a value to a [Place].
    /// A MIR [Place] is an L-value (i.e. the LHS of an assignment).
    ///
    /// In Kani, we slightly optimize the special case for Unit and don't assign anything.
    pub(crate) fn codegen_expr_to_place_stable(
        &mut self,
        place: &Place,
        expr: Expr,
        loc: Location,
    ) -> Stmt {
        if self.place_ty_stable(place).kind().is_unit() {
            expr.as_stmt(loc)
        } else {
            unwrap_or_return_codegen_unimplemented_stmt!(
                self,
                self.codegen_place_stable(place, loc)
            )
            .goto_expr
            .assign(expr, loc)
        }
    }
}
