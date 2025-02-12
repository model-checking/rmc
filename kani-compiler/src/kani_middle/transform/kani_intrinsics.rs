// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
//! Module responsible for generating code for a few Kani intrinsics.
//!
//! These intrinsics have code that depend on information from the compiler, such as type layout
//! information; thus, they are implemented as a transformation pass where their body get generated
//! by the transformation.

use crate::args::ExtraChecks;
use crate::kani_middle::abi::LayoutOf;
use crate::kani_middle::attributes::KaniAttributes;
use crate::kani_middle::kani_functions::{KaniFunction, KaniIntrinsic, KaniModel};
use crate::kani_middle::transform::body::{
    CheckType, InsertPosition, MutableBody, SourceInstruction,
};
use crate::kani_middle::transform::check_uninit::PointeeInfo;
use crate::kani_middle::transform::check_uninit::{
    PointeeLayout, mk_layout_operand, resolve_mem_init_fn,
};
use crate::kani_middle::transform::check_values::{build_limits, ty_validity_per_offset};
use crate::kani_middle::transform::{TransformPass, TransformationType};
use crate::kani_queries::QueryDb;
use rustc_middle::ty::TyCtxt;
use stable_mir::mir::mono::Instance;
use stable_mir::mir::{
    AggregateKind, BasicBlock, BinOp, Body, ConstOperand, Local, Mutability, Operand, Place,
    RETURN_LOCAL, Rvalue, Statement, StatementKind, Terminator, TerminatorKind, UnOp, UnwindAction,
};
use stable_mir::target::MachineInfo;
use stable_mir::ty::{
    AdtDef, FnDef, GenericArgKind, GenericArgs, MirConst, RigidTy, Ty, TyKind, UintTy,
};
use std::collections::HashMap;
use std::fmt::Debug;
use std::str::FromStr;
use tracing::{debug, trace};

/// Generate the body for a few Kani intrinsics.
#[derive(Debug)]
pub struct IntrinsicGeneratorPass {
    unsupported_check_type: CheckType,
    /// Used to cache FnDef lookups for models and Kani intrinsics.
    kani_defs: HashMap<KaniFunction, FnDef>,
    /// Whether the user enabled uninitialized memory checks when they invoked Kani.
    enable_uninit: bool,
}

impl TransformPass for IntrinsicGeneratorPass {
    fn transformation_type() -> TransformationType
    where
        Self: Sized,
    {
        TransformationType::Stubbing
    }

    fn is_enabled(&self, _query_db: &QueryDb) -> bool
    where
        Self: Sized,
    {
        true
    }

    /// Transform the function body by inserting checks one-by-one.
    /// For every unsafe dereference or a transmute operation, we check all values are valid.
    fn transform(&mut self, tcx: TyCtxt, body: Body, instance: Instance) -> (bool, Body) {
        trace!(function=?instance.name(), "transform");
        let attributes = KaniAttributes::for_instance(tcx, instance);
        if let Some(kani_intrinsic) =
            attributes.fn_marker().and_then(|name| KaniIntrinsic::from_str(name.as_str()).ok())
        {
            match kani_intrinsic {
                KaniIntrinsic::CheckedAlignOf => (true, self.checked_align_of(body, instance)),
                KaniIntrinsic::CheckedSizeOf => (true, self.checked_size_of(body, instance)),
                KaniIntrinsic::IsInitialized => (true, self.is_initialized_body(body)),
                KaniIntrinsic::ValidValue => (true, self.valid_value_body(body)),
                // The former two are handled in contracts pass for now, while the latter is handled in the the automatic harness pass.
                KaniIntrinsic::WriteAny
                | KaniIntrinsic::AnyModifies
                | KaniIntrinsic::AutomaticHarness => (false, body),
            }
        } else {
            (false, body)
        }
    }
}

impl IntrinsicGeneratorPass {
    pub fn new(unsupported_check_type: CheckType, queries: &QueryDb) -> Self {
        let enable_uninit = queries.args().ub_check.contains(&ExtraChecks::Uninit);
        let kani_defs = queries.kani_functions().clone();
        debug!(?kani_defs, ?enable_uninit, "IntrinsicGeneratorPass::new");
        IntrinsicGeneratorPass { unsupported_check_type, enable_uninit, kani_defs }
    }

    /// Generate the body for valid value. Which should be something like:
    ///
    /// ```
    /// pub fn has_valid_value<T>(ptr: *const T) -> bool {
    ///     let mut ret = true;
    ///     let bytes = ptr as *const u8;
    ///     for req in requirements {
    ///         ret &= in_range(bytes, req);
    ///     }
    ///     ret
    /// }
    /// ```
    fn valid_value_body(&self, body: Body) -> Body {
        let mut new_body = MutableBody::from(body);
        new_body.clear_body(TerminatorKind::Return);

        // Initialize return variable with True.
        let ret_var = RETURN_LOCAL;
        let mut terminator = SourceInstruction::Terminator { bb: 0 };
        let span = new_body.locals()[ret_var].span;
        let assign = StatementKind::Assign(
            Place::from(ret_var),
            Rvalue::Use(Operand::Constant(ConstOperand {
                span,
                user_ty: None,
                const_: MirConst::from_bool(true),
            })),
        );
        let stmt = Statement { kind: assign, span };
        new_body.insert_stmt(stmt, &mut terminator, InsertPosition::Before);
        let machine_info = MachineInfo::target();

        // The first and only argument type.
        let arg_ty = new_body.locals()[1].ty;
        let TyKind::RigidTy(RigidTy::RawPtr(target_ty, _)) = arg_ty.kind() else { unreachable!() };
        let validity = ty_validity_per_offset(&machine_info, target_ty, 0);
        match validity {
            Ok(ranges) if ranges.is_empty() => {
                // Nothing to check
            }
            Ok(ranges) => {
                // Given the pointer argument, check for possible invalid ranges.
                let rvalue = Rvalue::Use(Operand::Move(Place::from(1)));
                for range in ranges {
                    let result =
                        build_limits(&mut new_body, &range, rvalue.clone(), &mut terminator);
                    let rvalue = Rvalue::BinaryOp(
                        BinOp::BitAnd,
                        Operand::Move(Place::from(ret_var)),
                        Operand::Move(Place::from(result)),
                    );
                    let assign = StatementKind::Assign(Place::from(ret_var), rvalue);
                    let stmt = Statement { kind: assign, span };
                    new_body.insert_stmt(stmt, &mut terminator, InsertPosition::Before);
                }
            }
            Err(msg) => {
                // We failed to retrieve all the valid ranges.
                let reason = format!(
                    "Kani currently doesn't support checking validity of `{target_ty}`. {msg}"
                );
                new_body.insert_check(
                    &self.unsupported_check_type,
                    &mut terminator,
                    InsertPosition::Before,
                    None,
                    &reason,
                );
            }
        }
        new_body.into()
    }

    /// Generate the body for `is_initialized`, which looks like the following
    ///
    /// ```
    /// pub fn is_initialized<T>(ptr: *const T, len: usize) -> bool {
    ///     let layout = ... // Byte mask representing the layout of T.
    ///     __kani_mem_init_sm_get(ptr, layout, len)
    /// }
    /// ```
    fn is_initialized_body(&mut self, body: Body) -> Body {
        let mut new_body = MutableBody::from(body);
        new_body.clear_body(TerminatorKind::Return);
        let ret_var = RETURN_LOCAL;
        let mut source = SourceInstruction::Terminator { bb: 0 };

        // Short-circut if uninitialized memory checks are not enabled.
        if !self.enable_uninit {
            // Initialize return variable with True.
            let span = new_body.locals()[ret_var].span;
            let assign = StatementKind::Assign(
                Place::from(ret_var),
                Rvalue::Use(Operand::Constant(ConstOperand {
                    span,
                    user_ty: None,
                    const_: MirConst::from_bool(true),
                })),
            );
            new_body.insert_stmt(
                Statement { kind: assign, span },
                &mut source,
                InsertPosition::Before,
            );
            return new_body.into();
        }

        // Instead of injecting the instrumentation immediately, collect it into a list of
        // statements and a terminator to construct a basic block and inject it at the end.
        let mut statements = vec![];

        // The first argument type.
        let arg_ty = new_body.locals()[1].ty;
        // Sanity check: since CBMC memory object primitives only accept pointers, need to
        // ensure the correct type.
        let TyKind::RigidTy(RigidTy::RawPtr(pointee_ty, _)) = arg_ty.kind() else { unreachable!() };
        // Calculate pointee layout for byte-by-byte memory initialization checks.
        let pointee_info = PointeeInfo::from_ty(pointee_ty);
        match pointee_info {
            Ok(pointee_info) => {
                match pointee_info.layout() {
                    PointeeLayout::Sized { layout } => {
                        if layout.is_empty() {
                            // Encountered a ZST, so we can short-circut here.
                            // Initialize return variable with True.
                            let span = new_body.locals()[ret_var].span;
                            let assign = StatementKind::Assign(
                                Place::from(ret_var),
                                Rvalue::Use(Operand::Constant(ConstOperand {
                                    span,
                                    user_ty: None,
                                    const_: MirConst::from_bool(true),
                                })),
                            );
                            new_body.insert_stmt(
                                Statement { kind: assign, span },
                                &mut source,
                                InsertPosition::Before,
                            );
                            return new_body.into();
                        }
                        let is_ptr_initialized_instance = resolve_mem_init_fn(
                            *self.kani_defs.get(&KaniModel::IsPtrInitialized.into()).unwrap(),
                            layout.len(),
                            *pointee_info.ty(),
                        );
                        let layout_operand =
                            mk_layout_operand(&mut new_body, &mut statements, &mut source, &layout);

                        let terminator = Terminator {
                            kind: TerminatorKind::Call {
                                func: Operand::Copy(Place::from(new_body.new_local(
                                    is_ptr_initialized_instance.ty(),
                                    source.span(new_body.blocks()),
                                    Mutability::Not,
                                ))),
                                args: vec![Operand::Copy(Place::from(1)), layout_operand],
                                destination: Place::from(ret_var),
                                target: Some(0), // The current value does not matter, since it will be overwritten in add_bb.
                                unwind: UnwindAction::Terminate,
                            },
                            span: source.span(new_body.blocks()),
                        };
                        // Construct the basic block and insert it into the body.
                        new_body.insert_bb(
                            BasicBlock { statements, terminator },
                            &mut source,
                            InsertPosition::Before,
                        );
                    }
                    PointeeLayout::Slice { element_layout } => {
                        // Since `str`` is a separate type, need to differentiate between [T] and str.
                        let (slicee_ty, intrinsic) = match pointee_info.ty().kind() {
                            TyKind::RigidTy(RigidTy::Slice(slicee_ty)) => {
                                (slicee_ty, KaniModel::IsSlicePtrInitialized.into())
                            }
                            TyKind::RigidTy(RigidTy::Str) => {
                                (Ty::unsigned_ty(UintTy::U8), KaniModel::IsStrPtrInitialized.into())
                            }
                            _ => unreachable!(),
                        };
                        let is_ptr_initialized_instance = resolve_mem_init_fn(
                            *self.kani_defs.get(&intrinsic).unwrap(),
                            element_layout.len(),
                            slicee_ty,
                        );
                        let layout_operand = mk_layout_operand(
                            &mut new_body,
                            &mut statements,
                            &mut source,
                            &element_layout,
                        );
                        let terminator = Terminator {
                            kind: TerminatorKind::Call {
                                func: Operand::Copy(Place::from(new_body.new_local(
                                    is_ptr_initialized_instance.ty(),
                                    source.span(new_body.blocks()),
                                    Mutability::Not,
                                ))),
                                args: vec![Operand::Copy(Place::from(1)), layout_operand],
                                destination: Place::from(ret_var),
                                target: Some(0), // The current value does not matter, since it will be overwritten in add_bb.
                                unwind: UnwindAction::Terminate,
                            },
                            span: source.span(new_body.blocks()),
                        };
                        // Construct the basic block and insert it into the body.
                        new_body.insert_bb(
                            BasicBlock { statements, terminator },
                            &mut source,
                            InsertPosition::Before,
                        );
                    }
                    PointeeLayout::TraitObject => {
                        let reason: &str = "Kani does not support reasoning about memory initialization of pointers to trait objects.";

                        new_body.insert_check(
                            &self.unsupported_check_type,
                            &mut source,
                            InsertPosition::Before,
                            None,
                            &reason,
                        );
                    }
                    PointeeLayout::Union { .. } => {
                        let reason: &str =
                            "Kani does not yet support using initialization predicates on unions.";

                        new_body.insert_check(
                            &self.unsupported_check_type,
                            &mut source,
                            InsertPosition::Before,
                            None,
                            &reason,
                        );
                    }
                };
            }
            Err(reason) => {
                // We failed to retrieve the type layout.
                let reason = format!(
                    "Kani currently doesn't support checking memory initialization for pointers to `{pointee_ty}. {reason}",
                );
                new_body.insert_check(
                    &self.unsupported_check_type,
                    &mut source,
                    InsertPosition::Before,
                    None,
                    &reason,
                );
            }
        }
        new_body.into()
    }

    /// Generate the body for retrieving the size of a val starting from its raw pointer.
    ///
    /// The body generated will depend on the type of the pointer.
    ///
    /// For sized type, this will generate:
    /// ```mir
    ///     _0: Option<usize>;
    ///     _1: *const T;
    ///    bb0:
    ///     _0 = Some(<const_size>);
    ///     return
    /// ```
    ///
    /// For types with foreign tails, this will generate a `None` value.
    ///
    /// For types with trait and slice tails, gather information about the type and invoke
    /// `size_of_dyn_object` and `size_of_slice_object` respectively. E.g.::
    /// ```
    ///     _0: Option<usize>;
    ///     _1: *const T;
    ///    bb0:
    ///     _0 = size_of_dyn_object(_1, <head_sz>, <head_align>);
    ///    bb1:
    ///     return
    /// ```
    fn checked_size_of(&mut self, body: Body, instance: Instance) -> Body {
        // Get information about the pointer passed as an argument.
        let ptr_arg = body.arg_locals().first().expect("Expected a pointer argument");
        let ptr_ty = ptr_arg.ty;
        let TyKind::RigidTy(RigidTy::RawPtr(pointee_ty, _)) = ptr_ty.kind() else {
            unreachable!("Expected a pointer argument, but got {ptr_ty}")
        };
        let pointee_layout = LayoutOf::new(pointee_ty);
        debug!(?ptr_ty, ?pointee_layout, "checked_size_of");

        // Get information about the return value (`Option<usize>`).
        let ret_ty = body.ret_local().ty;
        let TyKind::RigidTy(RigidTy::Adt(option_def, option_args)) = ret_ty.kind() else {
            unreachable!("Expected `Option<usize>` as return but found `{ret_ty}`")
        };

        // Modify the body according to the type of pointer.
        let mut new_body = MutableBody::from(body);
        new_body.clear_body(TerminatorKind::Return);
        let mut source = SourceInstruction::Terminator { bb: 0 };
        let span = source.span(new_body.blocks());
        if pointee_layout.is_sized() {
            // Return Some(<size>);
            let val_op = new_body.new_uint_operand(
                pointee_layout.size_of().unwrap() as _,
                UintTy::Usize,
                span,
            );
            let ret_val = build_some(option_def, option_args, val_op);
            new_body.assign_to(
                Place::from(RETURN_LOCAL),
                ret_val,
                &mut source,
                InsertPosition::Before,
            );
        } else if pointee_layout.has_trait_tail() {
            // Return `size_of_dyn_object::<T, U>(ptr, head_size, head_align)`.
            let tail_ty = pointee_layout.unsized_tail().unwrap();
            let mut instance_args = instance.args(); // This should contain `T` already.
            instance_args.0.push(GenericArgKind::Type(tail_ty)); // Now push the tail type `U`.
            let ptr = Operand::Copy(Place::from(Local::from(1usize)));
            let head_size =
                new_body.new_uint_operand(pointee_layout.size_of_head() as _, UintTy::Usize, span);
            let head_align =
                new_body.new_uint_operand(pointee_layout.align_of_head() as _, UintTy::Usize, span);
            let operands = vec![ptr, head_size, head_align];
            self.return_model(
                &mut new_body,
                &mut source,
                KaniModel::SizeOfDynObject,
                &instance_args,
                operands,
            );
        } else if pointee_layout.has_slice_tail() {
            // Return `size_of_slice_object::<T, U>(len, elem_size, head_size, align)`.
            let elem_ty = pointee_layout.unsized_tail_elem_ty().unwrap();
            let elem_layout = LayoutOf::new(elem_ty);
            assert!(elem_layout.is_sized());

            let elem_size =
                new_body.new_uint_operand(elem_layout.size_of().unwrap() as _, UintTy::Usize, span);
            let head_size =
                new_body.new_uint_operand(pointee_layout.size_of_head() as _, UintTy::Usize, span);
            let align = new_body.new_uint_operand(
                pointee_layout.align_of().unwrap() as _,
                UintTy::Usize,
                span,
            );
            let ptr = Operand::Copy(Place::from(Local::from(1usize)));
            let len_local = new_body.insert_assignment(
                Rvalue::UnaryOp(UnOp::PtrMetadata, ptr),
                &mut source,
                InsertPosition::Before,
            );
            let len_op = Operand::Move(Place::from(len_local));
            let operands = vec![len_op, elem_size, head_size, align];
            self.return_model(
                &mut new_body,
                &mut source,
                KaniModel::SizeOfSliceObject,
                &instance.args(),
                operands,
            );
        } else {
            // Cannot compute size of foreign types. Return `None`.
            assert!(
                pointee_layout.has_foreign_tail(),
                "Expected foreign, but found `{:?}` tail instead.",
                pointee_layout.unsized_tail()
            );
            let ret_val = build_none(option_def, option_args);
            new_body.assign_to(
                Place::from(RETURN_LOCAL),
                ret_val,
                &mut source,
                InsertPosition::Before,
            );
        }
        new_body.into()
    }

    /// Generate the body for retrieving the alignment of the pointed to object if possible.
    ///
    /// The body generated will depend on the type.
    ///
    /// For sized type, and types with slice tails, the alignment can be computed statically, and
    /// this will generate:
    /// ```mir
    ///     _0: Option<usize>;
    ///     _1: *const T;
    ///    bb0:
    ///     _0 = Some(<align>);
    ///     return
    /// ```
    ///
    /// For types with trait tail, invoke `align_of_dyn_portion`:
    /// ```
    ///     _0: Option<usize>;
    ///     _1: *const T;
    ///    bb0:
    ///     _0 = align_of_dyn_object(_1, <head_align>);
    ///    bb1:
    ///     return
    /// ```
    ///
    /// For types with foreign tails, this will return `None`.
    fn checked_align_of(&mut self, body: Body, instance: Instance) -> Body {
        // Get information about the pointer passed as an argument.
        let ptr_arg = body.arg_locals().first().expect("Expected a pointer argument");
        let ptr_ty = ptr_arg.ty;
        let TyKind::RigidTy(RigidTy::RawPtr(pointee_ty, _)) = ptr_ty.kind() else {
            unreachable!("Expected a pointer argument, but got {ptr_ty}")
        };
        let pointee_layout = LayoutOf::new(pointee_ty);
        debug!(?ptr_ty, "align_of_raw");

        // Get information about the return value (Option).
        let ret_ty = body.ret_local().ty;
        let TyKind::RigidTy(RigidTy::Adt(option_def, option_args)) = ret_ty.kind() else {
            unreachable!("Expected `Option<usize>` as return but found `{ret_ty}`")
        };

        // Modify the body according to the type of pointer.
        let mut new_body = MutableBody::from(body);
        new_body.clear_body(TerminatorKind::Return);
        let mut source = SourceInstruction::Terminator { bb: 0 };
        let span = source.span(new_body.blocks());
        if let Some(align) = pointee_layout.align_of() {
            let val_op = new_body.new_uint_operand(align as _, UintTy::Usize, span);
            let ret_val = build_some(option_def, option_args, val_op);
            new_body.assign_to(
                Place::from(RETURN_LOCAL),
                ret_val,
                &mut source,
                InsertPosition::Before,
            );
        } else if pointee_layout.has_trait_tail() {
            // Return `align_of_dyn_object::<T, U>(ptr, head_align)`.
            let head_align =
                new_body.new_uint_operand(pointee_layout.align_of_head() as _, UintTy::Usize, span);
            let tail_ty = pointee_layout.unsized_tail().unwrap();
            let mut args = instance.args(); // This already contains `T`.
            args.0.push(GenericArgKind::Type(tail_ty)); // Now push the tail type `U`.
            let operands = vec![Operand::Copy(Place::from(Local::from(1usize))), head_align];
            self.return_model(
                &mut new_body,
                &mut source,
                KaniModel::AlignOfDynObject,
                &args,
                operands,
            );
        } else {
            // Cannot compute size of foreign types. Return None!
            assert!(
                pointee_layout.has_foreign_tail(),
                "Expected foreign, but found `{:?}` tail instead.",
                pointee_layout.unsized_tail()
            );
            let ret_val = build_none(option_def, option_args);
            new_body.assign_to(
                Place::from(RETURN_LOCAL),
                ret_val,
                &mut source,
                InsertPosition::Before,
            );
        }
        new_body.into()
    }

    fn return_model(
        &mut self,
        new_body: &mut MutableBody,
        mut source: &mut SourceInstruction,
        model: KaniModel,
        args: &GenericArgs,
        operands: Vec<Operand>,
    ) {
        let def = self.kani_defs.get(&model.into()).unwrap();
        let size_of_dyn = Instance::resolve(*def, args).unwrap();
        new_body.insert_call(
            &size_of_dyn,
            &mut source,
            InsertPosition::Before,
            operands,
            Place::from(RETURN_LOCAL),
        );
    }
}

/// Build an Rvalue `Some(val)`.
/// Since the variants of `Option` are `Some(val)` and `None`, we know we've found the `Some` variant when we find the first variant with a field.
fn build_some(option: AdtDef, args: GenericArgs, val_op: Operand) -> Rvalue {
    let var_idx = option
        .variants_iter()
        .find_map(|var| (!var.fields().is_empty()).then_some(var.idx))
        .unwrap();
    Rvalue::Aggregate(AggregateKind::Adt(option, var_idx, args, None, None), vec![val_op])
}

/// Build an Rvalue `None`.
/// Since the variants of `Option` are `Some(val)` and `None`, we know we've found the `None` variant when we find the first variant without fields.
fn build_none(option: AdtDef, args: GenericArgs) -> Rvalue {
    let var_idx =
        option.variants_iter().find_map(|var| var.fields().is_empty().then_some(var.idx)).unwrap();
    Rvalue::Aggregate(AggregateKind::Adt(option, var_idx, args, None, None), vec![])
}
