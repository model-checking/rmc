// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//
//! Implement a transformation pass that instrument the code to detect possible UB due to
//! the generation of an invalid value.
//!
//! This pass highly depend on Rust type layouts. For more details, see:
//! <https://doc.rust-lang.org/reference/type-layout.html>
//!
//! For that, we traverse the function body and look for unsafe operations that may generate
//! invalid values. For each operation found, we add checks to ensure the value is valid.
//!
//! Note: There is some redundancy in the checks that could be optimized. Example:
//!   1. We could merge the invalid values by the offset.
//!   2. We could avoid checking places that have been checked before.
use crate::args::ExtraChecks;
use crate::kani_middle::transform::body::{
    CheckType, InsertPosition, MutableBody, SourceInstruction,
};
use crate::kani_middle::transform::check_values::SourceOp::UnsupportedCheck;
use crate::kani_middle::transform::{TransformPass, TransformationType};
use crate::kani_queries::QueryDb;
use rustc_middle::ty::{Const, TyCtxt};
use rustc_smir::rustc_internal;
use stable_mir::CrateDef;
use stable_mir::abi::{FieldsShape, Scalar, TagEncoding, ValueAbi, VariantsShape, WrappingRange};
use stable_mir::mir::mono::Instance;
use stable_mir::mir::visit::{Location, PlaceContext, PlaceRef};
use stable_mir::mir::{
    AggregateKind, BasicBlockIdx, BinOp, Body, CastKind, FieldIdx, Local, LocalDecl, MirVisitor,
    Mutability, NonDivergingIntrinsic, Operand, Place, ProjectionElem, RawPtrKind, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};
use stable_mir::target::{MachineInfo, MachineSize};
use stable_mir::ty::{AdtKind, IndexedVal, RigidTy, Span, Ty, TyKind, UintTy};
use std::fmt::Debug;
use strum_macros::AsRefStr;
use tracing::{debug, trace};

/// Instrument the code with checks for invalid values.
#[derive(Debug)]
pub struct ValidValuePass {
    pub safety_check_type: CheckType,
    pub unsupported_check_type: CheckType,
}

impl TransformPass for ValidValuePass {
    fn transformation_type() -> TransformationType
    where
        Self: Sized,
    {
        TransformationType::Instrumentation
    }

    fn is_enabled(&self, query_db: &QueryDb) -> bool
    where
        Self: Sized,
    {
        let args = query_db.args();
        args.ub_check.contains(&ExtraChecks::Validity)
    }

    /// Transform the function body by inserting checks one-by-one.
    /// For every unsafe dereference or a transmute operation, we check all values are valid.
    fn transform(&mut self, tcx: TyCtxt, body: Body, instance: Instance) -> (bool, Body) {
        trace!(function=?instance.name(), "transform");
        let mut new_body = MutableBody::from(body);
        let orig_len = new_body.blocks().len();
        // Do not cache body.blocks().len() since it will change as we add new checks.
        for bb_idx in 0..new_body.blocks().len() {
            let Some(candidate) =
                CheckValueVisitor::find_next(tcx, &new_body, bb_idx, bb_idx >= orig_len)
            else {
                continue;
            };
            self.build_check(&mut new_body, candidate);
        }
        (orig_len != new_body.blocks().len(), new_body.into())
    }
}

impl ValidValuePass {
    fn build_check(&self, body: &mut MutableBody, instruction: UnsafeInstruction) {
        debug!(?instruction, "build_check");
        let mut source = instruction.source;
        for operation in instruction.operations {
            match operation {
                SourceOp::BytesValidity { ranges, target_ty, rvalue } => {
                    let value = body.insert_assignment(rvalue, &mut source, InsertPosition::Before);
                    let rvalue_ptr = Rvalue::AddressOf(RawPtrKind::Const, Place::from(value));
                    for range in ranges {
                        let result = build_limits(body, &range, rvalue_ptr.clone(), &mut source);
                        let msg =
                            format!("Undefined Behavior: Invalid value of type `{target_ty}`",);
                        body.insert_check(
                            &self.safety_check_type,
                            &mut source,
                            InsertPosition::Before,
                            Some(result),
                            &msg,
                        );
                    }
                }
                SourceOp::DerefValidity { pointee_ty, rvalue, ranges } => {
                    for range in ranges {
                        let result = build_limits(body, &range, rvalue.clone(), &mut source);
                        let msg =
                            format!("Undefined Behavior: Invalid value of type `{pointee_ty}`",);
                        body.insert_check(
                            &self.safety_check_type,
                            &mut source,
                            InsertPosition::Before,
                            Some(result),
                            &msg,
                        );
                    }
                }
                SourceOp::UnsupportedCheck { check, ty } => {
                    let reason = format!(
                        "Kani currently doesn't support checking validity of `{check}` for `{ty}`",
                    );
                    self.unsupported_check(body, &mut source, &reason);
                }
            }
        }
    }

    fn unsupported_check(
        &self,
        body: &mut MutableBody,
        source: &mut SourceInstruction,
        reason: &str,
    ) {
        body.insert_check(
            &self.unsupported_check_type,
            source,
            InsertPosition::Before,
            None,
            reason,
        );
    }
}

fn move_local(local: Local) -> Operand {
    Operand::Move(Place::from(local))
}

fn uint_ty(bytes: usize) -> UintTy {
    match bytes {
        1 => UintTy::U8,
        2 => UintTy::U16,
        4 => UintTy::U32,
        8 => UintTy::U64,
        16 => UintTy::U128,
        _ => unreachable!("Unexpected size: {bytes}"),
    }
}

/// Represent a requirement for the value stored in the given offset.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ValidValueReq {
    /// Offset in bytes.
    offset: usize,
    /// Size of this requirement.
    size: MachineSize,
    /// The range restriction is represented by a Scalar.
    valid_range: ValidityRange,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum ValidityRange {
    /// The value validity fits in a single value range.
    /// This includes cases where the full range is covered.
    Single(WrappingRange),
    /// The validity includes more than one value range.
    /// Currently, this is only the case for `char`, which has two ranges.
    /// If more cases come up, we could turn this into a vector instead.
    Multiple([WrappingRange; 2]),
}

// TODO: Optimize checks by merging requirements whenever possible.
// There are a few cases that would need to be cover:
// 1- Ranges intersection is the same as one of the ranges (or both).
// 2- Ranges intersection is a new valid range.
// 3- Ranges intersection is a combination of two new ranges.
// 4- Intersection is empty.
impl ValidValueReq {
    /// Only a type with `ValueAbi::Scalar` and `ValueAbi::ScalarPair` can be directly assigned an
    /// invalid value directly.
    ///
    /// It's not possible to define a `rustc_layout_scalar_valid_range_*` to any other structure.
    /// Note that this annotation only applies to the first scalar in the layout.
    pub fn try_from_ty(machine_info: &MachineInfo, ty: Ty) -> Option<ValidValueReq> {
        if ty.kind().is_char() {
            Some(ValidValueReq {
                offset: 0,
                size: MachineSize::from_bits(size_of::<char>() * 8),
                valid_range: ValidityRange::Multiple([
                    WrappingRange { start: 0, end: 0xD7FF },
                    WrappingRange { start: 0xE000, end: char::MAX.into() },
                ]),
            })
        } else {
            let shape = ty.layout().unwrap().shape();
            match shape.abi {
                ValueAbi::Scalar(Scalar::Initialized { value, valid_range })
                | ValueAbi::ScalarPair(Scalar::Initialized { value, valid_range }, _) => {
                    Some(ValidValueReq {
                        offset: 0,
                        size: value.size(machine_info),
                        valid_range: ValidityRange::Single(valid_range),
                    })
                }
                ValueAbi::Scalar(_)
                | ValueAbi::ScalarPair(_, _)
                | ValueAbi::Uninhabited
                | ValueAbi::Vector { .. }
                | ValueAbi::Aggregate { .. } => None,
            }
        }
    }

    /// Check if range is full.
    pub fn is_full(&self) -> bool {
        if let ValidityRange::Single(valid_range) = self.valid_range {
            valid_range.is_full(self.size).unwrap()
        } else {
            false
        }
    }

    /// Check if this range contains `other` range.
    ///
    /// I.e., `scalar_2` ⊆ `scalar_1`
    pub fn contains(&self, other: &ValidValueReq) -> bool {
        assert_eq!(self.size, other.size);
        match (&self.valid_range, &other.valid_range) {
            (ValidityRange::Single(this_range), ValidityRange::Single(other_range)) => {
                range_contains(this_range, other_range, self.size)
            }
            (ValidityRange::Multiple(this_ranges), ValidityRange::Single(other_range)) => {
                range_contains(&this_ranges[0], other_range, self.size)
                    || range_contains(&this_ranges[1], other_range, self.size)
            }
            (ValidityRange::Single(this_range), ValidityRange::Multiple(other_ranges)) => {
                range_contains(this_range, &other_ranges[0], self.size)
                    && range_contains(this_range, &other_ranges[1], self.size)
            }
            (ValidityRange::Multiple(this_ranges), ValidityRange::Multiple(other_ranges)) => {
                let contains = (range_contains(&this_ranges[0], &other_ranges[0], self.size)
                    || range_contains(&this_ranges[1], &other_ranges[0], self.size))
                    && (range_contains(&this_ranges[0], &other_ranges[1], self.size)
                        || range_contains(&this_ranges[1], &other_ranges[1], self.size));
                // Multiple today only cover `char` case.
                debug_assert!(
                    contains,
                    "Expected validity of `char` for Multiple ranges. Found: {self:?}, {other:?}"
                );
                contains
            }
        }
    }
}

/// Check if range `r1` contains range `r2`.
///
/// I.e., `r2` ⊆ `r1`
fn range_contains(r1: &WrappingRange, r2: &WrappingRange, sz: MachineSize) -> bool {
    match (r1.wraps_around(), r2.wraps_around()) {
        (true, true) | (false, false) => r1.start <= r2.start && r1.end >= r2.end,
        (true, false) => r1.start <= r2.start || r1.end >= r2.end,
        (false, true) => r1.is_full(sz).unwrap(),
    }
}

#[derive(AsRefStr, Clone, Debug)]
enum SourceOp {
    /// Validity checks are done on a byte level when the Rvalue can generate invalid value.
    ///
    /// This variant tracks a location that is valid for its current type, but it may not be
    /// valid for the given location in target type. This happens for:
    ///  - Transmute
    ///  - Field assignment
    ///  - Aggregate assignment
    ///  - Union Access
    ///
    /// Each range is a pair of offset and scalar that represents the valid values.
    /// Note that the same offset may have multiple ranges that may require being joined.
    BytesValidity { target_ty: Ty, rvalue: Rvalue, ranges: Vec<ValidValueReq> },

    /// Similar to BytesValidity, but it stores any dereference that may be unsafe.
    ///
    /// This can happen for:
    ///  - Raw pointer dereference
    DerefValidity { pointee_ty: Ty, rvalue: Rvalue, ranges: Vec<ValidValueReq> },

    /// Represents a range check Kani currently does not support.
    ///
    /// This will translate into an assertion failure with an unsupported message.
    /// There are many corner cases with the usage of #[rustc_layout_scalar_valid_range_*]
    /// attribute. Such as valid ranges that do not intersect or enumeration with variants
    /// with niche.
    ///
    /// Supporting all cases require significant work, and it is unlikely to exist in real world
    /// code. To be on the sound side, we just emit an unsupported check, and users will need to
    /// disable the check in person, and create a feature request for their case.
    ///
    /// TODO: Consider replacing the assertion(false) by an unsupported operation that emits a
    /// compilation warning.
    UnsupportedCheck { check: String, ty: Ty },
}

/// The unsafe instructions that may generate invalid values.
/// We need to instrument all operations to ensure the instruction is safe.
#[derive(Clone, Debug)]
struct UnsafeInstruction {
    /// The instruction that depends on the potentially invalid value.
    source: SourceInstruction,
    /// The unsafe operations that may cause an invalid value in this instruction.
    operations: Vec<SourceOp>,
}

/// Extract any source that may potentially trigger UB due to the generation of an invalid value.
///
/// Generating an invalid value requires an unsafe operation, however, in MIR, it
/// may just be represented as a regular assignment.
///
/// Thus, we have to instrument every assignment to an object that has niche and that the source
/// is an object of a different source, e.g.:
///   - Aggregate assignment
///   - Transmute
///   - MemCopy
///   - Cast
struct CheckValueVisitor<'a, 'b> {
    tcx: TyCtxt<'b>,
    locals: &'a [LocalDecl],
    /// Whether we should skip the next instruction, since it might've been instrumented already.
    /// When we instrument an instruction, we partition the basic block, and the instruction that
    /// may trigger UB becomes the first instruction of the basic block, which we need to skip
    /// later.
    skip_next: bool,
    /// The instruction being visited at a given point.
    current: SourceInstruction,
    /// The target instruction that should be verified.
    pub target: Option<UnsafeInstruction>,
    /// The basic block being visited.
    bb: BasicBlockIdx,
    /// Machine information needed to calculate Niche.
    machine: MachineInfo,
}

impl<'a, 'b> CheckValueVisitor<'a, 'b> {
    fn find_next(
        tcx: TyCtxt<'b>,
        body: &'a MutableBody,
        bb: BasicBlockIdx,
        skip_first: bool,
    ) -> Option<UnsafeInstruction> {
        let mut visitor = CheckValueVisitor {
            tcx,
            locals: body.locals(),
            skip_next: skip_first,
            current: SourceInstruction::Statement { idx: 0, bb },
            target: None,
            bb,
            machine: MachineInfo::target(),
        };
        visitor.visit_basic_block(&body.blocks()[bb]);
        visitor.target
    }

    fn push_target(&mut self, op: SourceOp) {
        let target = self
            .target
            .get_or_insert_with(|| UnsafeInstruction { source: self.current, operations: vec![] });
        target.operations.push(op);
    }
}

impl MirVisitor for CheckValueVisitor<'_, '_> {
    fn visit_statement(&mut self, stmt: &Statement, location: Location) {
        if self.skip_next {
            self.skip_next = false;
        } else if self.target.is_none() {
            // Leave it as an exhaustive match to be notified when a new kind is added.
            match &stmt.kind {
                StatementKind::Intrinsic(NonDivergingIntrinsic::CopyNonOverlapping(copy)) => {
                    // Source is a *const T and it must be safe for read.
                    // TODO: Implement value check.
                    self.push_target(SourceOp::UnsupportedCheck {
                        check: "copy_nonoverlapping".to_string(),
                        ty: copy.src.ty(self.locals).unwrap(),
                    });
                }
                StatementKind::Assign(place, rvalue) => {
                    // First check rvalue.
                    self.super_statement(stmt, location);
                    // Then check the destination place.
                    let ranges = assignment_check_points(
                        &self.machine,
                        self.locals,
                        place,
                        rvalue.ty(self.locals).unwrap(),
                    );
                    if !ranges.is_empty() {
                        self.push_target(SourceOp::BytesValidity {
                            target_ty: self.locals[place.local].ty,
                            rvalue: rvalue.clone(),
                            ranges,
                        });
                    }
                }
                StatementKind::FakeRead(_, _)
                | StatementKind::SetDiscriminant { .. }
                | StatementKind::Deinit(_)
                | StatementKind::StorageLive(_)
                | StatementKind::StorageDead(_)
                | StatementKind::Retag(_, _)
                | StatementKind::PlaceMention(_)
                | StatementKind::AscribeUserType { .. }
                | StatementKind::Coverage(_)
                | StatementKind::ConstEvalCounter
                | StatementKind::Intrinsic(NonDivergingIntrinsic::Assume(_))
                | StatementKind::Nop => self.super_statement(stmt, location),
            }
        }

        let SourceInstruction::Statement { idx, bb } = self.current else { unreachable!() };
        self.current = SourceInstruction::Statement { idx: idx + 1, bb };
    }
    fn visit_terminator(&mut self, term: &Terminator, location: Location) {
        if !(self.skip_next || self.target.is_some()) {
            self.current = SourceInstruction::Terminator { bb: self.bb };
            // Leave it as an exhaustive match to be notified when a new kind is added.
            match &term.kind {
                TerminatorKind::Call { func, args, .. } => {
                    // Note: For transmute, both Src and Dst must be valid type.
                    // In this case, we need to save the Dst, and invoke super_terminator.
                    self.super_terminator(term, location);
                    match intrinsic_name(self.locals, func).as_deref() {
                        Some("write_bytes") => {
                            // The write bytes intrinsic may trigger UB in safe code.
                            // pub unsafe fn write_bytes<T>(dst: *mut T, val: u8, count: usize)
                            // <https://doc.rust-lang.org/stable/core/intrinsics/fn.write_bytes.html>
                            // This is an over-approximation since writing an invalid value is
                            // not UB, only reading it will be.
                            assert_eq!(
                                args.len(),
                                3,
                                "Unexpected number of arguments for `write_bytes`"
                            );
                            let TyKind::RigidTy(RigidTy::RawPtr(target_ty, Mutability::Mut)) =
                                args[0].ty(self.locals).unwrap().kind()
                            else {
                                unreachable!()
                            };
                            let validity = ty_validity_per_offset(&self.machine, target_ty, 0);
                            match validity {
                                Ok(ranges) if ranges.is_empty() => {}
                                Ok(ranges) => {
                                    let sz = rustc_internal::stable(Const::from_target_usize(
                                        self.tcx,
                                        target_ty.layout().unwrap().shape().size.bytes() as u64,
                                    ));
                                    self.push_target(SourceOp::BytesValidity {
                                        target_ty,
                                        rvalue: Rvalue::Repeat(args[1].clone(), sz),
                                        ranges,
                                    })
                                }
                                _ => self.push_target(SourceOp::UnsupportedCheck {
                                    check: "write_bytes".to_string(),
                                    ty: target_ty,
                                }),
                            }
                        }
                        Some("transmute") | Some("transmute_copy") => {
                            unreachable!("Should've been lowered")
                        }
                        _ => {}
                    }
                }
                TerminatorKind::Goto { .. }
                | TerminatorKind::SwitchInt { .. }
                | TerminatorKind::Resume
                | TerminatorKind::Abort
                | TerminatorKind::Return
                | TerminatorKind::Unreachable
                | TerminatorKind::Drop { .. }
                | TerminatorKind::Assert { .. }
                | TerminatorKind::InlineAsm { .. } => self.super_terminator(term, location),
            }
        }
    }

    fn visit_place(&mut self, place: &Place, ptx: PlaceContext, location: Location) {
        for (idx, elem) in place.projection.iter().enumerate() {
            let place_ref = PlaceRef { local: place.local, projection: &place.projection[..idx] };
            match elem {
                ProjectionElem::Deref => {
                    let ptr_ty = place_ref.ty(self.locals).unwrap();
                    if ptr_ty.kind().is_raw_ptr() {
                        let target_ty = elem.ty(ptr_ty).unwrap();
                        let validity = ty_validity_per_offset(&self.machine, target_ty, 0);
                        match validity {
                            Ok(ranges) if !ranges.is_empty() => {
                                self.push_target(SourceOp::DerefValidity {
                                    pointee_ty: target_ty,
                                    rvalue: Rvalue::Use(
                                        Operand::Copy(Place {
                                            local: place_ref.local,
                                            projection: place_ref.projection.to_vec(),
                                        })
                                        .clone(),
                                    ),
                                    ranges,
                                })
                            }
                            Err(_msg) => self.push_target(SourceOp::UnsupportedCheck {
                                check: "raw pointer dereference".to_string(),
                                ty: target_ty,
                            }),
                            _ => {}
                        }
                    }
                }
                ProjectionElem::Field(idx, target_ty) => {
                    if target_ty.kind().is_union()
                        && (!ptx.is_mutating() || place.projection.len() > idx + 1)
                    {
                        let validity = ty_validity_per_offset(&self.machine, *target_ty, 0);
                        match validity {
                            Ok(ranges) if !ranges.is_empty() => {
                                self.push_target(SourceOp::BytesValidity {
                                    target_ty: *target_ty,
                                    rvalue: Rvalue::Use(Operand::Copy(Place {
                                        local: place_ref.local,
                                        projection: place_ref.projection.to_vec(),
                                    })),
                                    ranges,
                                })
                            }
                            Err(_msg) => self.push_target(SourceOp::UnsupportedCheck {
                                check: "union access".to_string(),
                                ty: *target_ty,
                            }),
                            _ => {}
                        }
                    }
                }
                ProjectionElem::Downcast(_) => {}
                ProjectionElem::OpaqueCast(_) => {}
                ProjectionElem::Subtype(_) => {}
                ProjectionElem::Index(_)
                | ProjectionElem::ConstantIndex { .. }
                | ProjectionElem::Subslice { .. } => { /* safe */ }
            }
        }
        self.super_place(place, ptx, location)
    }

    fn visit_rvalue(&mut self, rvalue: &Rvalue, location: Location) {
        match rvalue {
            Rvalue::Cast(kind, op, dest_ty) => match kind {
                CastKind::PtrToPtr => {
                    // For mutable raw pointer, if the type we are casting to is less restrictive
                    // than the original type, writing to the pointer could generate UB if the
                    // value is ever read again using the original pointer.
                    let TyKind::RigidTy(RigidTy::RawPtr(dest_pointee_ty, Mutability::Mut)) =
                        dest_ty.kind()
                    else {
                        // We only care about *mut T as *mut U
                        return;
                    };
                    if dest_pointee_ty.kind().is_unit() {
                        // Ignore cast to *mut () since nothing can be written to it.
                        // This is a common pattern
                        return;
                    }

                    let src_ty = op.ty(self.locals).unwrap();
                    debug!(?src_ty, ?dest_ty, "visit_rvalue mutcast");
                    let TyKind::RigidTy(RigidTy::RawPtr(src_pointee_ty, _)) = src_ty.kind() else {
                        unreachable!()
                    };

                    if src_pointee_ty.kind().is_unit() {
                        // We cannot track what was the initial type. Thus, fail.
                        self.push_target(SourceOp::UnsupportedCheck {
                            check: "mutable cast".to_string(),
                            ty: src_ty,
                        });
                        return;
                    }

                    if let Ok(src_validity) =
                        ty_validity_per_offset(&self.machine, src_pointee_ty, 0)
                    {
                        if !src_validity.is_empty() {
                            if let Ok(dest_validity) =
                                ty_validity_per_offset(&self.machine, dest_pointee_ty, 0)
                            {
                                if dest_validity != src_validity {
                                    self.push_target(SourceOp::UnsupportedCheck {
                                        check: "mutable cast".to_string(),
                                        ty: src_ty,
                                    })
                                }
                            } else {
                                self.push_target(SourceOp::UnsupportedCheck {
                                    check: "mutable cast".to_string(),
                                    ty: *dest_ty,
                                })
                            }
                        }
                    } else {
                        self.push_target(SourceOp::UnsupportedCheck {
                            check: "mutable cast".to_string(),
                            ty: src_ty,
                        })
                    }
                }
                CastKind::Transmute => {
                    debug!(?dest_ty, "transmute");
                    // For transmute, we care about the destination type only.
                    // This could be optimized to only add a check if the requirements of the
                    // destination type are stricter than the source.
                    if let Ok(dest_validity) = ty_validity_per_offset(&self.machine, *dest_ty, 0) {
                        trace!(?dest_validity, "transmute");
                        if !dest_validity.is_empty() {
                            self.push_target(SourceOp::BytesValidity {
                                target_ty: *dest_ty,
                                rvalue: rvalue.clone(),
                                ranges: dest_validity,
                            })
                        }
                    } else {
                        self.push_target(SourceOp::UnsupportedCheck {
                            check: "transmute".to_string(),
                            ty: *dest_ty,
                        })
                    }
                }
                // `DynStar` is not currently supported in Kani.
                // TODO: https://github.com/model-checking/kani/issues/1784
                CastKind::DynStar => self.push_target(UnsupportedCheck {
                    check: "Dyn*".to_string(),
                    ty: (rvalue.ty(self.locals).unwrap()),
                }),
                CastKind::PointerExposeAddress
                | CastKind::PointerWithExposedProvenance
                | CastKind::PointerCoercion(_)
                | CastKind::IntToInt
                | CastKind::FloatToInt
                | CastKind::FloatToFloat
                | CastKind::IntToFloat
                | CastKind::FnPtrToPtr => {}
            },
            Rvalue::ShallowInitBox(_, _) => {
                // The contents of the box is considered uninitialized.
                // This should already be covered by the Assign detection.
            }
            Rvalue::Aggregate(kind, operands) => match kind {
                // If the aggregated structure has invalid value, this could generate invalid value.
                // But only if the operands don't have the exact same restrictions.
                // This happens today with the usage of `rustc_layout_scalar_valid_range_*`
                // attributes.
                // In this case, only the value of the first member in memory can be restricted,
                // thus, we only need to check the operand used to assign to the first in memory
                // field.
                AggregateKind::Adt(def, _variant, args, _, _) => {
                    if def.kind() == AdtKind::Struct {
                        let dest_ty = Ty::from_rigid_kind(RigidTy::Adt(*def, args.clone()));
                        if let Some(req) = ValidValueReq::try_from_ty(&self.machine, dest_ty)
                            && !req.is_full()
                        {
                            let dest_layout = dest_ty.layout().unwrap().shape();
                            let first_op =
                                first_aggregate_operand(dest_ty, &dest_layout.fields, operands);
                            let first_ty = first_op.ty(self.locals).unwrap();
                            // Rvalue must have same Abi layout except for range.
                            if !req.contains(
                                &ValidValueReq::try_from_ty(&self.machine, first_ty).unwrap(),
                            ) {
                                self.push_target(SourceOp::BytesValidity {
                                    target_ty: dest_ty,
                                    rvalue: Rvalue::Use(first_op),
                                    ranges: vec![req],
                                })
                            }
                        }
                    }
                }
                // Only aggregate value.
                AggregateKind::Array(_)
                | AggregateKind::Closure(_, _)
                | AggregateKind::Coroutine(_, _, _)
                | AggregateKind::CoroutineClosure(_, _)
                | AggregateKind::RawPtr(_, _)
                | AggregateKind::Tuple => {}
            },
            Rvalue::AddressOf(_, _)
            | Rvalue::BinaryOp(_, _, _)
            | Rvalue::CheckedBinaryOp(_, _, _)
            | Rvalue::CopyForDeref(_)
            | Rvalue::Discriminant(_)
            | Rvalue::Len(_)
            | Rvalue::Ref(_, _, _)
            | Rvalue::Repeat(_, _)
            | Rvalue::ThreadLocalRef(_)
            | Rvalue::NullaryOp(_, _)
            | Rvalue::UnaryOp(_, _)
            | Rvalue::Use(_) => {}
        }
        self.super_rvalue(rvalue, location);
    }
}

/// Gets the operand that corresponds to the assignment of the first sized field in memory.
///
/// The first field of a structure is the only one that can have extra value restrictions imposed
/// by `rustc_layout_scalar_valid_range_*` attributes.
///
/// Note: This requires at least one operand to be sized and there's a 1:1 match between operands
/// and field types.
fn first_aggregate_operand(dest_ty: Ty, dest_shape: &FieldsShape, operands: &[Operand]) -> Operand {
    let Some(first) = first_sized_field_idx(dest_ty, dest_shape) else { unreachable!() };
    operands[first].clone()
}

/// Index of the first non_1zst fields in memory order.
fn first_sized_field_idx(ty: Ty, shape: &FieldsShape) -> Option<FieldIdx> {
    if let TyKind::RigidTy(RigidTy::Adt(adt_def, args)) = ty.kind()
        && adt_def.kind() == AdtKind::Struct
    {
        let offset_order = shape.fields_by_offset_order();
        let fields = adt_def.variants_iter().next().unwrap().fields();
        offset_order
            .into_iter()
            .find(|idx| !fields[*idx].ty_with_args(&args).layout().unwrap().shape().is_1zst())
    } else {
        None
    }
}

/// An assignment to a field with invalid values is unsafe, and it may trigger UB if
/// the assigned value is invalid.
///
/// This can only happen to the first in memory sized field of a struct, and only if the field
/// type invalid range is a valid value for the rvalue type.
fn assignment_check_points(
    machine_info: &MachineInfo,
    locals: &[LocalDecl],
    place: &Place,
    rvalue_ty: Ty,
) -> Vec<ValidValueReq> {
    let mut ty = locals[place.local].ty;
    let Some(rvalue_range) = ValidValueReq::try_from_ty(machine_info, rvalue_ty) else {
        // Rvalue Abi must be Scalar / ScalarPair since destination must be Scalar / ScalarPair.
        return vec![];
    };
    let mut invalid_ranges = vec![];
    for proj in &place.projection {
        match proj {
            ProjectionElem::Field(field_idx, field_ty) => {
                let shape = ty.layout().unwrap().shape();
                if first_sized_field_idx(ty, &shape.fields) == Some(*field_idx)
                    && let Some(dest_valid) = ValidValueReq::try_from_ty(machine_info, ty)
                    && !dest_valid.is_full()
                    && dest_valid.size == rvalue_range.size
                {
                    if !dest_valid.contains(&rvalue_range) {
                        invalid_ranges.push(dest_valid)
                    }
                } else {
                    // Invalidate collected ranges so far since we are no longer in the path of
                    // the first element.
                    invalid_ranges.clear();
                }
                ty = *field_ty;
            }
            ProjectionElem::Deref
            | ProjectionElem::Index(_)
            | ProjectionElem::ConstantIndex { .. }
            | ProjectionElem::Subslice { .. }
            | ProjectionElem::Downcast(_)
            | ProjectionElem::OpaqueCast(_)
            | ProjectionElem::Subtype(_) => ty = proj.ty(ty).unwrap(),
        };
    }
    invalid_ranges
}

/// Retrieve the name of the intrinsic if this operand is an intrinsic.
///
/// Intrinsics can only be invoked directly, so we can safely ignore other operand types.
fn intrinsic_name(locals: &[LocalDecl], func: &Operand) -> Option<String> {
    let ty = func.ty(locals).unwrap();
    let TyKind::RigidTy(RigidTy::FnDef(def, args)) = ty.kind() else { return None };
    Instance::resolve(def, &args).unwrap().intrinsic_name()
}

/// Instrument MIR to check the value pointed by `rvalue_ptr` satisfies requirement `req`.
///
/// The MIR will do something equivalent to:
/// ```rust
///     let ptr = rvalue_ptr.byte_offset(req.offset);
///     let typed_ptr = ptr as *const Unsigned<req.size>; // Some unsigned type with length req.size
///     let value = unsafe { *typed_ptr };
///     req.valid_range.contains(value)
/// ```
pub fn build_limits(
    body: &mut MutableBody,
    req: &ValidValueReq,
    rvalue_ptr: Rvalue,
    source: &mut SourceInstruction,
) -> Local {
    let span = source.span(body.blocks());
    debug!(?req, ?rvalue_ptr, ?span, "build_limits");
    let primitive_ty = uint_ty(req.size.bytes());
    let orig_ptr = if req.offset != 0 {
        let start_ptr =
            move_local(body.insert_assignment(rvalue_ptr, source, InsertPosition::Before));
        let byte_ptr = move_local(body.insert_ptr_cast(
            start_ptr,
            Ty::unsigned_ty(UintTy::U8),
            Mutability::Not,
            source,
            InsertPosition::Before,
        ));
        let offset_const = body.new_uint_operand(req.offset as _, UintTy::Usize, span);
        let offset = move_local(body.insert_assignment(
            Rvalue::Use(offset_const),
            source,
            InsertPosition::Before,
        ));
        move_local(body.insert_binary_op(
            BinOp::Offset,
            byte_ptr,
            offset,
            source,
            InsertPosition::Before,
        ))
    } else {
        move_local(body.insert_assignment(rvalue_ptr, source, InsertPosition::Before))
    };
    let value_ptr = body.insert_ptr_cast(
        orig_ptr,
        Ty::unsigned_ty(primitive_ty),
        Mutability::Not,
        source,
        InsertPosition::Before,
    );
    let value = Operand::Copy(Place { local: value_ptr, projection: vec![ProjectionElem::Deref] });
    match &req.valid_range {
        ValidityRange::Single(range) => {
            build_single_limit(body, range, source, span, primitive_ty, value)
        }
        ValidityRange::Multiple([range1, range2]) => {
            // Build `let valid = range1.contains(value) || range2.contains(value);
            let cond1 = build_single_limit(body, range1, source, span, primitive_ty, value.clone());
            let cond2 = build_single_limit(body, range2, source, span, primitive_ty, value);
            body.insert_binary_op(
                BinOp::BitOr,
                move_local(cond1),
                move_local(cond2),
                source,
                InsertPosition::Before,
            )
        }
    }
}

fn build_single_limit(
    body: &mut MutableBody,
    range: &WrappingRange,
    source: &mut SourceInstruction,
    span: Span,
    primitive_ty: UintTy,
    value: Operand,
) -> Local {
    let start_const = body.new_uint_operand(range.start, primitive_ty, span);
    let end_const = body.new_uint_operand(range.end, primitive_ty, span);
    let start_result = body.insert_binary_op(
        BinOp::Ge,
        value.clone(),
        start_const,
        source,
        InsertPosition::Before,
    );
    let end_result =
        body.insert_binary_op(BinOp::Le, value, end_const, source, InsertPosition::Before);
    if range.wraps_around() {
        // valid >= start || valid <= end
        body.insert_binary_op(
            BinOp::BitOr,
            move_local(start_result),
            move_local(end_result),
            source,
            InsertPosition::Before,
        )
    } else {
        // valid >= start && valid <= end
        body.insert_binary_op(
            BinOp::BitAnd,
            move_local(start_result),
            move_local(end_result),
            source,
            InsertPosition::Before,
        )
    }
}

/// Traverse the type and find all invalid values and their location in memory.
///
/// Not all values are currently supported. For those not supported, we return Error.
pub fn ty_validity_per_offset(
    machine_info: &MachineInfo,
    ty: Ty,
    current_offset: usize,
) -> Result<Vec<ValidValueReq>, String> {
    let layout = ty.layout().unwrap().shape();
    let ty_req = || {
        if let Some(mut req) = ValidValueReq::try_from_ty(machine_info, ty)
            && !req.is_full()
        {
            req.offset = current_offset;
            vec![req]
        } else {
            vec![]
        }
    };
    match layout.fields {
        FieldsShape::Primitive => Ok(ty_req()),
        FieldsShape::Array { stride, count } if count > 0 => {
            let TyKind::RigidTy(RigidTy::Array(elem_ty, _)) = ty.kind() else { unreachable!() };
            let elem_validity = ty_validity_per_offset(machine_info, elem_ty, current_offset)?;
            let mut result = vec![];
            if !elem_validity.is_empty() {
                for idx in 0..count {
                    let idx: usize = idx.try_into().unwrap();
                    let elem_offset = idx * stride.bytes();
                    let mut next_validity = elem_validity
                        .iter()
                        .cloned()
                        .map(|mut req| {
                            req.offset += elem_offset;
                            req
                        })
                        .collect::<Vec<_>>();
                    result.append(&mut next_validity)
                }
            }
            Ok(result)
        }
        FieldsShape::Arbitrary { ref offsets } => {
            match ty.kind().rigid().expect(&format!("unexpected type: {ty:?}")) {
                RigidTy::Adt(def, args) => {
                    match def.kind() {
                        AdtKind::Enum => {
                            // Support basic enumeration forms
                            let ty_variants = def.variants();
                            match layout.variants {
                                VariantsShape::Empty => Ok(vec![]),
                                VariantsShape::Single { index } => {
                                    // Only one variant is reachable. This behaves like a struct.
                                    let fields = ty_variants[index.to_index()].fields();
                                    let mut fields_validity = vec![];
                                    for idx in layout.fields.fields_by_offset_order() {
                                        let field_offset = offsets[idx].bytes();
                                        let field_ty = fields[idx].ty_with_args(&args);
                                        fields_validity.append(&mut ty_validity_per_offset(
                                            machine_info,
                                            field_ty,
                                            field_offset + current_offset,
                                        )?);
                                    }
                                    Ok(fields_validity)
                                }
                                VariantsShape::Multiple {
                                    tag_encoding: TagEncoding::Niche { .. },
                                    ..
                                } => {
                                    Err(format!("Unsupported Enum `{}` check", def.trimmed_name()))?
                                }
                                VariantsShape::Multiple { variants, .. } => {
                                    let enum_validity = ty_req();
                                    let mut fields_validity = vec![];
                                    for (index, variant) in variants.iter().enumerate() {
                                        let fields = ty_variants[index].fields();
                                        for field_idx in variant.fields.fields_by_offset_order() {
                                            let field_offset = offsets[field_idx].bytes();
                                            let field_ty = fields[field_idx].ty_with_args(&args);
                                            fields_validity.append(&mut ty_validity_per_offset(
                                                machine_info,
                                                field_ty,
                                                field_offset + current_offset,
                                            )?);
                                        }
                                    }
                                    if fields_validity.is_empty() {
                                        Ok(enum_validity)
                                    } else {
                                        Err(format!(
                                            "Unsupported Enum `{}` check",
                                            def.trimmed_name()
                                        ))
                                    }
                                }
                            }
                        }
                        AdtKind::Union => unreachable!(),
                        AdtKind::Struct => {
                            // If the struct range has niche add that.
                            let mut struct_validity = ty_req();
                            let fields = def.variants_iter().next().unwrap().fields();
                            for idx in layout.fields.fields_by_offset_order() {
                                let field_offset = offsets[idx].bytes();
                                let field_ty = fields[idx].ty_with_args(&args);
                                struct_validity.append(&mut ty_validity_per_offset(
                                    machine_info,
                                    field_ty,
                                    field_offset + current_offset,
                                )?);
                            }
                            Ok(struct_validity)
                        }
                    }
                }
                RigidTy::Pat(base_ty, ..) => {
                    // This is similar to a structure with one field and with niche defined.
                    let mut pat_validity = ty_req();
                    pat_validity.append(&mut ty_validity_per_offset(machine_info, *base_ty, 0)?);
                    Ok(pat_validity)
                }
                RigidTy::Tuple(tys) => {
                    let mut tuple_validity = vec![];
                    for idx in layout.fields.fields_by_offset_order() {
                        let field_offset = offsets[idx].bytes();
                        let field_ty = tys[idx];
                        tuple_validity.append(&mut ty_validity_per_offset(
                            machine_info,
                            field_ty,
                            field_offset + current_offset,
                        )?);
                    }
                    Ok(tuple_validity)
                }
                RigidTy::Bool
                | RigidTy::Char
                | RigidTy::Int(_)
                | RigidTy::Uint(_)
                | RigidTy::Float(_)
                | RigidTy::Never => {
                    unreachable!("Expected primitive layout for {ty:?}")
                }
                RigidTy::Str | RigidTy::Slice(_) | RigidTy::Array(_, _) => {
                    unreachable!("Expected array layout for {ty:?}")
                }
                RigidTy::RawPtr(_, _) | RigidTy::Ref(_, _, _) => {
                    // Fat pointer has arbitrary shape.
                    Ok(ty_req())
                }
                RigidTy::FnDef(_, _)
                | RigidTy::FnPtr(_)
                | RigidTy::Closure(_, _)
                | RigidTy::Coroutine(_, _, _)
                | RigidTy::CoroutineClosure(_, _)
                | RigidTy::CoroutineWitness(_, _)
                | RigidTy::Foreign(_)
                | RigidTy::Dynamic(_, _, _) => Err(format!("Unsupported {ty:?}")),
            }
        }
        FieldsShape::Union(_) | FieldsShape::Array { .. } => {
            /* Anything is valid */
            Ok(vec![])
        }
    }
}
