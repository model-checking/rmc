// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use itertools::Itertools;
use std::io::Write;

use crate::kani_queries::QueryDb;
use boogie_ast::boogie_program::{
    BinaryOp, BoogieProgram, DataType, DataTypeConstructor, Expr, Literal, Parameter, Procedure,
    Stmt, Type, UnaryOp,
};
use rustc_data_structures::fx::FxHashMap;
use rustc_middle::mir::interpret::Scalar;
use rustc_middle::mir::traversal::reverse_postorder;
use rustc_middle::mir::{
    BasicBlock, BasicBlockData, BinOp, Body, Const as mirConst, ConstOperand, ConstValue,
    HasLocalDecls, Local, Operand, Place, ProjectionElem, Rvalue, Statement, StatementKind,
    SwitchTargets, Terminator, TerminatorKind, UnOp, VarDebugInfoContents,
};
use rustc_middle::span_bug;
use rustc_middle::ty::layout::{
    HasParamEnv, HasTyCtxt, LayoutError, LayoutOf, LayoutOfHelpers, TyAndLayout,
};
use rustc_middle::ty::{self, Instance, IntTy, List, Ty, TyCtxt, UintTy};
use rustc_span::Span;
use rustc_target::abi::{HasDataLayout, TargetDataLayout};
use std::collections::hash_map::Entry;
use strum::IntoEnumIterator;
use tracing::{debug, debug_span, trace};

use super::kani_intrinsic::get_kani_intrinsic;
use super::smt_builtins::{smt_builtin_binop, SmtBvBuiltin};

const UNBOUNDED_ARRAY: &'static str = "$Array";

/// A context that provides the main methods for translating MIR constructs to
/// Boogie and stores what has been codegen so far
pub struct BoogieCtx<'tcx> {
    /// the typing context
    pub tcx: TyCtxt<'tcx>,
    /// a snapshot of the query values. The queries shouldn't change at this point,
    /// so we just keep a copy.
    pub queries: QueryDb,
    /// the Boogie program
    program: BoogieProgram,
}

impl<'tcx> BoogieCtx<'tcx> {
    pub fn new(tcx: TyCtxt<'tcx>, queries: QueryDb) -> BoogieCtx<'tcx> {
        let mut program = BoogieProgram::new();

        // TODO: The current functions in the preamble should be added lazily instead
        Self::add_preamble(&mut program);

        BoogieCtx { tcx, queries, program }
    }

    fn add_preamble(program: &mut BoogieProgram) {
        // Add SMT bv builtins
        for bv_builtin in SmtBvBuiltin::iter() {
            program.add_function(smt_builtin_binop(
                &bv_builtin,
                bv_builtin.smt_op_name(),
                bv_builtin.is_predicate(),
            ));
        }

        // Add unbounded array
        let name = String::from(UNBOUNDED_ARRAY);
        let constructor = DataTypeConstructor::new(
            name.clone(),
            vec![
                Parameter::new(
                    String::from("data"),
                    Type::map(Type::Bv(64), Type::parameter(String::from("T"))),
                ),
                Parameter::new(String::from("len"), Type::Bv(64)),
            ],
        );
        let unbounded_array_data_type =
            DataType::new(name.clone(), vec![String::from("T")], vec![constructor]);
        program.add_datatype(unbounded_array_data_type);
    }

    /// Codegen a function into a Boogie procedure.
    /// Returns `None` if the function is a hook.
    pub fn codegen_function(&self, instance: Instance<'tcx>) -> Option<Procedure> {
        debug!(?instance, "boogie_codegen_function");
        if get_kani_intrinsic(self.tcx, instance).is_some() {
            debug!("skipping kani intrinsic `{instance}`");
            return None;
        }
        let mut fcx = FunctionCtx::new(self, instance);
        let mut decl = fcx.codegen_declare_variables();
        let body = fcx.codegen_body();
        decl.push(body);
        Some(Procedure::new(
            self.tcx.symbol_name(instance).name.to_string(),
            vec![],
            vec![],
            None,
            Stmt::block(decl),
        ))
    }

    pub fn add_procedure(&mut self, procedure: Procedure) {
        self.program.add_procedure(procedure);
    }

    /// Write the program to the given writer
    pub fn write<T: Write>(&self, writer: &mut T) -> std::io::Result<()> {
        self.program.write_to(writer)?;
        Ok(())
    }
}

pub(crate) struct FunctionCtx<'a, 'tcx> {
    bcx: &'a BoogieCtx<'tcx>,
    instance: Instance<'tcx>,
    mir: &'a Body<'tcx>,
    /// Maps from local to the name of the corresponding Boogie variable.
    local_names: FxHashMap<Local, String>,
    /// A map to keep track of the source of each borrow. This is an ugly hack
    /// that only works in very special cases, more specifically where an
    /// explicit variable is borrowed, e.g.
    /// ```
    /// let b = &mut x;
    /// ````
    /// In this case, the map will contain an entry that maps `b` to `x`
    pub(crate) ref_to_expr: FxHashMap<Place<'tcx>, Expr>,
}

impl<'a, 'tcx> FunctionCtx<'a, 'tcx> {
    pub fn new(bcx: &'a BoogieCtx<'tcx>, instance: Instance<'tcx>) -> FunctionCtx<'a, 'tcx> {
        // create names for all locals
        let mut local_names = FxHashMap::default();
        let mut name_occurrences: FxHashMap<String, usize> = FxHashMap::default();
        let mir = bcx.tcx.instance_mir(instance.def);
        let ldecls = mir.local_decls();
        for local in ldecls.indices() {
            let debug_info = mir.var_debug_info.iter().find(|info| match info.value {
                VarDebugInfoContents::Place(p) => p.local == local && p.projection.len() == 0,
                VarDebugInfoContents::Const(_) => false,
            });
            let name = if let Some(debug_info) = debug_info {
                let base_name = format!("{}", debug_info.name);
                let entry = name_occurrences.entry(base_name.clone());
                let name = match entry {
                    Entry::Occupied(mut o) => {
                        let occ = o.get_mut();
                        let index = *occ;
                        *occ += 1;
                        format!("{base_name}_{}", index)
                    }
                    Entry::Vacant(v) => {
                        v.insert(1);
                        base_name
                    }
                };
                name
            } else {
                format!("{local:?}")
            };
            local_names.insert(local, name);
        }
        Self { bcx, instance, mir, local_names, ref_to_expr: FxHashMap::default() }
    }

    fn codegen_declare_variables(&self) -> Vec<Stmt> {
        let ldecls = self.mir.local_decls();
        let decls: Vec<Stmt> = ldecls
            .indices()
            .filter_map(|lc| {
                let typ = self.instance.instantiate_mir_and_normalize_erasing_regions(
                    self.tcx(),
                    ty::ParamEnv::reveal_all(),
                    ty::EarlyBinder::bind(ldecls[lc].ty),
                );
                // skip ZSTs
                if self.layout_of(typ).is_zst() {
                    return None;
                }
                debug!(?lc, ?typ, "codegen_declare_variables");
                let name = self.local_name(lc).clone();
                // skip the declaration of mutable references (e.g. `let mut _9: &mut i32;`)
                if let ty::Ref(_, _, m) = typ.kind() {
                    if m.is_mut() {
                        return None;
                    }
                }
                let boogie_type = self.codegen_type(typ);
                Some(Stmt::Decl { name, typ: boogie_type })
            })
            .collect();
        decls
    }

    fn codegen_type(&self, ty: Ty<'tcx>) -> Type {
        trace!(typ=?ty, "codegen_type");
        match ty.kind() {
            ty::Bool => Type::Bool,
            ty::Int(ity) => Type::Bv(ity.bit_width().unwrap_or(64).try_into().unwrap()),
            ty::Uint(uty) => Type::Bv(uty.bit_width().unwrap_or(64).try_into().unwrap()),
            ty::Tuple(types) => {
                // TODO: Only handles first element of tuple for now (e.g.
                // ignores overflow field of an addition and only takes the
                // result field)
                self.codegen_type(types.iter().next().unwrap())
            }
            ty::Adt(def, args) => {
                let name = format!("{def:?}");
                if name == "kani::array::Array" {
                    let fields = def.all_fields();
                    //let mut field_types: Vec<Type> = fields.filter_map(|f| {
                    //    let typ = f.ty(self.tcx(), args);
                    //    self.layout_of(typ).is_zst().then(|| self.codegen_type(typ))
                    //}).collect();
                    //assert_eq!(field_types.len(), 1);
                    //let typ = field_types.pop().unwrap();
                    let phantom_data_field = fields
                        .filter(|f| self.layout_of(f.ty(self.tcx(), args)).is_zst())
                        .exactly_one()
                        .unwrap_or_else(|_| panic!());
                    let phantom_data_type = phantom_data_field.ty(self.tcx(), args);
                    assert!(phantom_data_type.is_phantom_data());
                    let field_type = args.types().exactly_one().unwrap_or_else(|_| panic!());
                    let typ = self.codegen_type(field_type);
                    Type::user_defined(String::from(UNBOUNDED_ARRAY), vec![typ])
                } else {
                    todo!()
                }
            }
            ty::Ref(_r, ty, m) => {
                if m.is_not() {
                    return self.codegen_type(*ty);
                }
                todo!()
            }
            _ => todo!(),
        }
    }

    fn codegen_body(&mut self) -> Stmt {
        let statements: Vec<Stmt> =
            reverse_postorder(self.mir).map(|(bb, bbd)| self.codegen_block(bb, bbd)).collect();
        Stmt::block(statements)
    }

    fn codegen_block(&mut self, bb: BasicBlock, bbd: &BasicBlockData<'tcx>) -> Stmt {
        debug!(?bb, ?bbd, "codegen_block");
        // the first statement should be labelled. if there is no statements, then the
        // terminator should be labelled.
        let statements = match bbd.statements.len() {
            0 => {
                let term = bbd.terminator();
                let tcode = self.codegen_terminator(term);
                vec![tcode]
            }
            _ => {
                let mut statements: Vec<Stmt> =
                    bbd.statements.iter().map(|stmt| self.codegen_statement(stmt)).collect();

                let term = self.codegen_terminator(bbd.terminator());
                statements.push(term);
                statements
            }
        };
        Stmt::labelled_block(format!("{bb:?}"), statements)
    }

    fn codegen_statement(&mut self, stmt: &Statement<'tcx>) -> Stmt {
        match &stmt.kind {
            StatementKind::Assign(box (place, rvalue)) => {
                debug!(?place, ?rvalue, "codegen_statement");
                let place_name = self.local_name(place.local).clone();
                if let Rvalue::Ref(_, _, rhs) = rvalue {
                    let expr = self.codegen_place(rhs);
                    self.ref_to_expr.insert(*place, expr);
                    Stmt::Skip
                } else if is_deref(place) {
                    // lookup the place itself
                    debug!(?self.ref_to_expr, ?place, ?place.local, "codegen_statement_assign_deref");
                    let empty_projection = List::empty();
                    let place = Place { local: place.local, projection: empty_projection };
                    let expr = self.ref_to_expr.get(&place).unwrap();
                    let rv = self.codegen_rvalue(rvalue);
                    let asgn = Stmt::Assignment { target: expr.to_string(), value: rv.1 };
                    add_statement(rv.0, asgn)
                } else {
                    let rv = self.codegen_rvalue(rvalue);
                    // assignment statement
                    let asgn = Stmt::Assignment { target: place_name, value: rv.1 };
                    // add it to other statements generated while creating the rvalue (if any)
                    add_statement(rv.0, asgn)
                }
            }
            StatementKind::FakeRead(..)
            | StatementKind::SetDiscriminant { .. }
            | StatementKind::Deinit(..)
            | StatementKind::StorageLive(..)
            | StatementKind::StorageDead(..)
            | StatementKind::Retag(..)
            | StatementKind::PlaceMention(..)
            | StatementKind::AscribeUserType(..)
            | StatementKind::Coverage(..)
            | StatementKind::Intrinsic(..)
            | StatementKind::ConstEvalCounter
            | StatementKind::Nop => todo!(),
        }
    }

    /// Codegen an rvalue. Returns the expression for the rvalue and an optional
    /// statement for any possible checks instrumented for the rvalue expression
    fn codegen_rvalue(&self, rvalue: &Rvalue<'tcx>) -> (Option<Stmt>, Expr) {
        debug!(rvalue=?rvalue, "codegen_rvalue");
        match rvalue {
            Rvalue::Use(operand) => (None, self.codegen_operand(operand)),
            Rvalue::UnaryOp(op, operand) => self.codegen_unary_op(op, operand),
            Rvalue::BinaryOp(binop, box (lhs, rhs)) => self.codegen_binary_op(binop, lhs, rhs),
            Rvalue::CheckedBinaryOp(binop, box (ref e1, ref e2)) => {
                // TODO: handle overflow check
                self.codegen_binary_op(binop, e1, e2)
            }
            _ => todo!(),
        }
    }

    fn codegen_unary_op(&self, op: &UnOp, operand: &Operand<'tcx>) -> (Option<Stmt>, Expr) {
        debug!(op=?op, operand=?operand, "codegen_unary_op");
        let o = self.codegen_operand(operand);
        let expr = match op {
            UnOp::Not => {
                // TODO: can this be used for bit-level inversion as well?
                Expr::UnaryOp { op: UnaryOp::Not, operand: Box::new(o) }
            }
            UnOp::Neg => todo!(),
        };
        (None, expr)
    }

    fn codegen_binary_op(
        &self,
        binop: &BinOp,
        lhs: &Operand<'tcx>,
        rhs: &Operand<'tcx>,
    ) -> (Option<Stmt>, Expr) {
        debug!(binop=?binop, "codegen_binary_op");
        let left = Box::new(self.codegen_operand(lhs));
        let right = Box::new(self.codegen_operand(rhs));
        let expr = match binop {
            BinOp::Eq => Expr::BinaryOp { op: BinaryOp::Eq, left, right },
            BinOp::AddUnchecked | BinOp::Add => {
                let left_type = self.operand_ty(lhs);
                if self.operand_ty(rhs) != left_type {
                    todo!("Addition of different types is not yet supported");
                } else {
                    let bv_func = match left_type.kind() {
                        ty::Int(_) | ty::Uint(_) => SmtBvBuiltin::Add,
                        _ => todo!(),
                    };
                    Expr::function_call(bv_func.as_ref().to_owned(), vec![*left, *right])
                }
            }
            BinOp::Lt | BinOp::Ge => {
                let left_type = self.operand_ty(lhs);
                assert_eq!(left_type, self.operand_ty(rhs));
                let bv_func = match left_type.kind() {
                    ty::Int(_) => SmtBvBuiltin::SignedLessThan,
                    ty::Uint(_) => SmtBvBuiltin::UnsignedLessThan,
                    _ => todo!(),
                };
                let call = Expr::function_call(bv_func.as_ref().to_owned(), vec![*left, *right]);
                if let BinOp::Lt = binop { call } else { !call }
            }
            BinOp::Gt | BinOp::Le => {
                let left_type = self.operand_ty(lhs);
                assert_eq!(left_type, self.operand_ty(rhs));
                let bv_func = match left_type.kind() {
                    ty::Int(_) => SmtBvBuiltin::SignedGreaterThan,
                    ty::Uint(_) => SmtBvBuiltin::UnsignedGreaterThan,
                    _ => todo!(),
                };
                let call = Expr::function_call(bv_func.as_ref().to_owned(), vec![*left, *right]);
                if let BinOp::Gt = binop { call } else { !call }
            }
            BinOp::BitAnd => {
                Expr::function_call(SmtBvBuiltin::And.as_ref().to_owned(), vec![*left, *right])
            }
            BinOp::BitOr => {
                Expr::function_call(SmtBvBuiltin::Or.as_ref().to_owned(), vec![*left, *right])
            }
            BinOp::Shr => {
                let left_ty = self.operand_ty(lhs);
                let right_ty = self.operand_ty(lhs);
                debug!(?left_ty, ?right_ty, "codegen_binary_op_shr");
                Expr::function_call(SmtBvBuiltin::Shr.as_ref().to_owned(), vec![*left, *right])
            }
            BinOp::Shl => {
                let left_ty = self.operand_ty(lhs);
                let right_ty = self.operand_ty(lhs);
                debug!(?left_ty, ?right_ty, "codegen_binary_op_shl");
                Expr::function_call(SmtBvBuiltin::Shl.as_ref().to_owned(), vec![*left, *right])
            }
            _ => todo!(),
        };
        (None, expr)
    }

    fn codegen_terminator(&mut self, term: &Terminator<'tcx>) -> Stmt {
        let _trace_span = debug_span!("CodegenTerminator", statement = ?term.kind).entered();
        debug!("handling terminator {:?}", term);
        match &term.kind {
            TerminatorKind::Call { func, args, destination, target, .. } => {
                self.codegen_funcall(func, args, destination, target, term.source_info.span)
            }
            TerminatorKind::Goto { target } => Stmt::Goto { label: format!("{target:?}") },
            TerminatorKind::Return => Stmt::Return,
            TerminatorKind::SwitchInt { discr, targets } => self.codegen_switch_int(discr, targets),
            TerminatorKind::Assert { .. } => Stmt::Skip, // TODO: ignore injection assertions for now
            _ => todo!(),
        }
    }

    fn codegen_funcall(
        &mut self,
        func: &Operand<'tcx>,
        args: &[Operand<'tcx>],
        destination: &Place<'tcx>,
        target: &Option<BasicBlock>,
        span: Span,
    ) -> Stmt {
        debug!(?func, ?args, ?destination, ?span, "codegen_funcall");
        let funct = self.operand_ty(func);
        // TODO: Only Kani intrinsics are handled currently
        match &funct.kind() {
            ty::FnDef(defid, substs) => {
                let instance = Instance::expect_resolve(
                    self.tcx(),
                    ty::ParamEnv::reveal_all(),
                    *defid,
                    substs,
                );

                if let Some(intrinsic) = get_kani_intrinsic(self.tcx(), instance) {
                    return self.codegen_kani_intrinsic(
                        intrinsic,
                        instance,
                        args,
                        *destination,
                        *target,
                        Some(span),
                    );
                }
                let _fargs = self.codegen_funcall_args(args);
                todo!()
            }
            _ => todo!(),
        }
    }

    fn codegen_switch_int(&self, discr: &Operand<'tcx>, targets: &SwitchTargets) -> Stmt {
        debug!(discr=?discr, targets=?targets, "codegen_switch_int");
        let op = self.codegen_operand(discr);
        if targets.all_targets().len() == 2 {
            let then = targets.iter().next().unwrap();
            let right = match self.operand_ty(discr).kind() {
                ty::Bool => Literal::Bool(then.0 != 0),
                ty::Uint(_) => Literal::bv(128, then.0.into()),
                _ => unreachable!(),
            };
            // model as an if
            return Stmt::If {
                condition: Expr::BinaryOp {
                    op: BinaryOp::Eq,
                    left: Box::new(op),
                    right: Box::new(Expr::Literal(right)),
                },
                body: Box::new(Stmt::Goto { label: format!("{:?}", then.1) }),
                else_body: Some(Box::new(Stmt::Goto {
                    label: format!("{:?}", targets.otherwise()),
                })),
            };
        }
        todo!()
    }

    fn codegen_funcall_args(&self, args: &[Operand<'tcx>]) -> Vec<Expr> {
        debug!(?args, "codegen_funcall_args");
        args.iter()
            .filter_map(|o| {
                let ty = self.operand_ty(o);
                // TODO: handle non-primitive types
                if ty.is_primitive() {
                    return Some(self.codegen_operand(o));
                }
                // TODO: ignore non-primitive arguments for now (e.g. `msg`
                // argument of `kani::assert`)
                None
            })
            .collect()
    }

    pub(crate) fn codegen_operand(&self, o: &Operand<'tcx>) -> Expr {
        trace!(operand=?o, "codegen_operand");
        // A MIR operand is either a constant (literal or `const` declaration)
        // or a place (being moved or copied for this operation).
        // An "operand" in MIR is the argument to an "Rvalue" (and is also used
        // by some statements.)
        match o {
            Operand::Copy(place) | Operand::Move(place) => self.codegen_place(place),
            Operand::Constant(c) => self.codegen_constant(c),
        }
    }

    pub(crate) fn codegen_place(&self, place: &Place<'tcx>) -> Expr {
        debug!(place=?place, "codegen_place");
        debug!(place.local=?place.local, "codegen_place");
        debug!(place.projection=?place.projection, "codegen_place");
        if let Some(expr) = self.ref_to_expr.get(place) {
            return expr.clone();
        }
        //let local_ty = self.mir.local_decls()[place.local].ty;
        let local = self.codegen_local(place.local);
        local
        //place.projection.iter().fold(local, |place, proj| {
        //    match proj {
        //        ProjectionElem::Index(i) => {
        //            let index = self.codegen_local(i);
        //            Expr::Index { base: Box::new(place), index: Box::new(index) }
        //        }
        //        ProjectionElem::Field(f, _t) => {
        //            debug!(ty=?local_ty, "codegen_place_fold");
        //            match local_ty.kind() {
        //                ty::Adt(def, _args) => {
        //                    let field_name = def.non_enum_variant().fields[f].name.to_string();
        //                    Expr::Field { base: Box::new(place), field: field_name }
        //                }
        //                ty::Tuple(_types) => {
        //                    // TODO: handle tuples
        //                    place
        //                }
        //                _ => todo!(),
        //            }
        //        }
        //        _ => {
        //            // TODO: handle
        //            place
        //        }
        //    }
        //})
    }

    fn codegen_local(&self, local: Local) -> Expr {
        // TODO: handle function definitions
        Expr::Symbol { name: self.local_name(local).clone() }
    }

    fn local_name(&self, local: Local) -> &String {
        &self.local_names[&local]
    }

    fn codegen_constant(&self, c: &ConstOperand<'tcx>) -> Expr {
        trace!(constant=?c, "codegen_constant");
        // TODO: monomorphize
        match c.const_ {
            mirConst::Val(val, ty) => self.codegen_constant_value(val, ty),
            _ => todo!(),
        }
    }

    fn codegen_constant_value(&self, val: ConstValue<'tcx>, ty: Ty<'tcx>) -> Expr {
        debug!(val=?val, "codegen_constant_value");
        match val {
            ConstValue::Scalar(s) => self.codegen_scalar(s, ty),
            _ => todo!(),
        }
    }

    fn codegen_scalar(&self, s: Scalar, ty: Ty<'tcx>) -> Expr {
        debug!(kind=?ty.kind(), "codegen_scalar");
        match (s, ty.kind()) {
            (Scalar::Int(_), ty::Bool) => Expr::Literal(Literal::Bool(s.to_bool().unwrap())),
            (Scalar::Int(_), ty::Int(it)) => match it {
                IntTy::I8 => Expr::Literal(Literal::bv(8, s.to_i8().unwrap().into())),
                IntTy::I16 => Expr::Literal(Literal::bv(16, s.to_i16().unwrap().into())),
                IntTy::I32 => Expr::Literal(Literal::bv(32, s.to_i32().unwrap().into())),
                IntTy::I64 => Expr::Literal(Literal::bv(64, s.to_i64().unwrap().into())),
                IntTy::I128 => Expr::Literal(Literal::bv(128, s.to_i128().unwrap().into())),
                IntTy::Isize => {
                    // TODO: get target width
                    Expr::Literal(Literal::bv(64, s.to_target_isize(self).unwrap().into()))
                }
            },
            (Scalar::Int(_), ty::Uint(it)) => match it {
                UintTy::U8 => Expr::Literal(Literal::bv(8, s.to_u8().unwrap().into())),
                UintTy::U16 => Expr::Literal(Literal::bv(16, s.to_u16().unwrap().into())),
                UintTy::U32 => Expr::Literal(Literal::bv(32, s.to_u32().unwrap().into())),
                UintTy::U64 => Expr::Literal(Literal::bv(64, s.to_u64().unwrap().into())),
                UintTy::U128 => Expr::Literal(Literal::bv(128, s.to_u128().unwrap().into())),
                UintTy::Usize => {
                    // TODO: get target width
                    Expr::Literal(Literal::bv(64, s.to_target_usize(self).unwrap().into()))
                }
            },
            _ => todo!(),
        }
    }

    fn operand_ty(&self, o: &Operand<'tcx>) -> Ty<'tcx> {
        // TODO: monomorphize
        o.ty(self.mir.local_decls(), self.bcx.tcx)
    }
}

impl<'a, 'tcx> LayoutOfHelpers<'tcx> for FunctionCtx<'a, 'tcx> {
    type LayoutOfResult = TyAndLayout<'tcx>;

    fn handle_layout_err(&self, err: LayoutError<'tcx>, span: Span, ty: Ty<'tcx>) -> ! {
        span_bug!(span, "failed to get layout for `{}`: {}", ty, err)
    }
}

impl<'a, 'tcx> HasParamEnv<'tcx> for FunctionCtx<'a, 'tcx> {
    fn param_env(&self) -> ty::ParamEnv<'tcx> {
        ty::ParamEnv::reveal_all()
    }
}

impl<'a, 'tcx> HasTyCtxt<'tcx> for FunctionCtx<'a, 'tcx> {
    fn tcx(&self) -> TyCtxt<'tcx> {
        self.bcx.tcx
    }
}

impl<'a, 'tcx> HasDataLayout for FunctionCtx<'a, 'tcx> {
    fn data_layout(&self) -> &TargetDataLayout {
        self.bcx.tcx.data_layout()
    }
}

/// Create a new statement that includes `s1` (if non-empty) and `s2`
fn add_statement(s1: Option<Stmt>, s2: Stmt) -> Stmt {
    match s1 {
        Some(s1) => match s1 {
            Stmt::Block { label, mut statements } => {
                statements.push(s2);
                Stmt::Block { label, statements }
            }
            _ => Stmt::block(vec![s1, s2]),
        },
        None => s2,
    }
}

fn is_deref(p: &Place<'_>) -> bool {
    let proj = p.projection;
    if proj.len() == 1 && proj.iter().next().unwrap() == ProjectionElem::Deref {
        return true;
    }
    false
}
