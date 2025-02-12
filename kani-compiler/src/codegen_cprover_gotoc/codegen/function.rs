// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This file contains functions related to codegenning MIR functions into gotoc

use crate::codegen_cprover_gotoc::GotocCtx;
use crate::codegen_cprover_gotoc::codegen::block::reverse_postorder;
use cbmc::InternString;
use cbmc::goto_program::{Expr, Stmt, Symbol};
use stable_mir::CrateDef;
use stable_mir::mir::mono::Instance;
use stable_mir::mir::{Body, Local};
use stable_mir::ty::{RigidTy, TyKind};
use std::collections::BTreeMap;
use tracing::{debug, debug_span};

/// Codegen MIR functions into gotoc
impl GotocCtx<'_> {
    /// Declare variables according to their index.
    /// - Index 0 represents the return value.
    /// - Indices [1, N] represent the function parameters where N is the number of parameters.
    /// - Indices that are greater than N represent local variables.
    fn codegen_declare_variables(&mut self, body: &Body) {
        let ldecls = body.local_decls();
        let num_args = body.arg_locals().len();
        for (lc, ldata) in ldecls {
            if Some(lc) == body.spread_arg() {
                // We have already added this local in the function prelude, so
                // skip adding it again here.
                continue;
            }
            let base_name = self.codegen_var_base_name(&lc);
            let name = self.codegen_var_name(&lc);
            let var_type = self.codegen_ty_stable(ldata.ty);
            let loc = self.codegen_span_stable(ldata.span);
            // Indices [1, N] represent the function parameters where N is the number of parameters.
            // Except that ZST fields are not included as parameters.
            let sym =
                Symbol::variable(name, base_name, var_type, self.codegen_span_stable(ldata.span))
                    .with_is_hidden(!self.is_user_variable(&lc))
                    .with_is_parameter((lc > 0 && lc <= num_args) && !self.is_zst_stable(ldata.ty));
            let sym_e = sym.to_expr();
            self.symbol_table.insert(sym);

            // Index 0 represents the return value, which does not need to be
            // declared in the first block
            if lc < 1 || lc > body.arg_locals().len() {
                let init = self.codegen_default_initializer(&sym_e);
                self.current_fn_mut().push_onto_block(Stmt::decl(sym_e, init, loc));
            }
        }
    }

    pub fn codegen_function(&mut self, instance: Instance) {
        let name = instance.mangled_name();
        let old_sym = self.symbol_table.lookup(&name).unwrap();

        let _trace_span = debug_span!("CodegenFunction", name = instance.name()).entered();
        if old_sym.is_function_definition() {
            debug!("Double codegen of {:?}", old_sym);
        } else {
            assert!(old_sym.is_function());
            let body = self.transformer.body(self.tcx, instance);
            self.set_current_fn(instance, &body);
            self.print_instance(instance, &body);
            self.codegen_function_prelude(&body);
            self.codegen_declare_variables(&body);

            // Get the order from internal body for now.
            reverse_postorder(&body).for_each(|bb| self.codegen_block(bb, &body.blocks[bb]));

            let loc = self.codegen_span_stable(instance.def.span());
            let stmts = self.current_fn_mut().extract_block();
            let goto_body = Stmt::block(stmts, loc);
            self.symbol_table.update_fn_declaration_with_definition(&name, goto_body);
            self.reset_current_fn();
        }
    }

    /// Codegen changes required due to the function ABI.
    /// We currently untuple arguments for RustCall ABI where the `spread_arg` is set.
    fn codegen_function_prelude(&mut self, body: &Body) {
        debug!(spread_arg=?body.spread_arg(), "codegen_function_prelude");
        if let Some(spread_arg) = body.spread_arg() {
            self.codegen_spread_arg(body, spread_arg);
        }
    }

    /// MIR functions have a `spread_arg` field that specifies whether the
    /// final argument to the function is "spread" at the LLVM/codegen level
    /// from a tuple into its individual components. (Used for the "rust-
    /// call" ABI, necessary because the function traits and closures cannot have an
    /// argument list in MIR that is both generic and variadic, so Rust
    /// allows a generic tuple).
    ///
    /// These tuples are used in the MIR to invoke a shim, and it's used in the shim body.
    ///
    /// The `spread_arg` represents the the local variable that is to be "spread"/untupled.
    /// However, the function body itself may refer to the members of
    /// the tuple instead of the individual spread parameters, so we need to add to the
    /// function prelude code that _retuples_, that is, writes the arguments
    /// back to a local tuple that can be used in the body.
    ///
    /// See:
    /// <https://rust-lang.zulipchat.com/#narrow/stream/182449-t-compiler.2Fhelp/topic/Determine.20untupled.20closure.20args.20from.20Instance.3F>
    fn codegen_spread_arg(&mut self, body: &Body, spread_arg: Local) {
        debug!(current=?self.current_fn().name(), "codegen_spread_arg");
        let spread_data = &body.locals()[spread_arg];
        let tup_ty = spread_data.ty;
        if self.is_zst_stable(tup_ty) {
            // No need to spread a ZST since it will be ignored.
            return;
        }

        let loc = self.codegen_span_stable(spread_data.span);

        // Get the function signature from MIR, _before_ we untuple
        let instance = self.current_fn().instance_stable();
        // Closures themselves will have their arguments already untupled,
        // see Zulip link above.
        assert!(
            !instance.ty().kind().is_closure(),
            "Unexpected spread arg `{}` set for closure `{}`",
            spread_arg,
            instance.name()
        );

        // When we codegen the function signature elsewhere, we will codegen the untupled version.
        // We then marshall the arguments into a local variable holding the expected tuple.
        // For a function with args f(a: t1, b: t2, c: t3), the tuple type will look like
        // ```
        //    struct T {
        //        0: t1,
        //        1: t2,
        //        2: t3,
        // }
        // ```
        // For e.g., in the test `tupled_closure.rs`, the tuple type looks like:
        // ```
        // struct _8098103865751214180
        // {
        //    unsigned long int 1;
        //    unsigned char 0;
        //    struct _3159196586427472662 2;
        // };
        // ```
        // Note how the compiler has reordered the fields to improve packing.
        let tup_type = self.codegen_ty_stable(tup_ty);

        // We need to marshall the arguments into the tuple
        // The arguments themselves have been tacked onto the explicit function paramaters by
        // the code in `pub fn fn_typ(&mut self) -> Type {` in `typ.rs`.
        // By convention, they are given the names `spread<i>`.
        // For e.g., in the test `tupled_closure.rs`, the actual function looks like
        // ```
        // unsigned long int _RNvYNvCscgV8bIzQQb7_14tupled_closure1hINtNtNtCsaGHNm3cehi1_4core3ops8function2FnThjINtNtBH_6option6OptionNtNtNtBH_3num7nonzero12NonZeroUsizeEEE4callB4_(
        //        unsigned long int (*var_1)(unsigned char, unsigned long int, struct _3159196586427472662),
        //        unsigned char spread_2,
        //        unsigned long int spread_3,
        //        struct _3159196586427472662 spread_4) {
        //  struct _8098103865751214180 var_2={ .1=spread_3, .0=spread_2, .2=spread_4 };
        //  unsigned long int var_0=(_RNvCscgV8bIzQQb7_14tupled_closure1h)(var_2.0, var_2.1, var_2.2);
        //  return var_0;
        // }
        // ```

        let TyKind::RigidTy(RigidTy::Tuple(args)) = tup_ty.kind() else {
            unreachable!("a function's spread argument must be a tuple")
        };
        let starting_idx = spread_arg;
        let marshalled_tuple_fields =
            BTreeMap::from_iter(args.iter().enumerate().map(|(arg_i, arg_t)| {
                // The components come at the end, so offset by the untupled length.
                // This follows the naming convention defined in `typ.rs`.
                let lc = arg_i + starting_idx;
                let (name, base_name) = self.codegen_spread_arg_name(&lc);
                let sym = Symbol::variable(name, base_name, self.codegen_ty_stable(*arg_t), loc)
                    .with_is_hidden(false)
                    .with_is_parameter(!self.is_zst_stable(*arg_t));
                // The spread arguments are additional function paramaters that are patched in
                // They are to the function signature added in the `fn_typ` function.
                // But they were never added to the symbol table, which we currently do here.
                // https://github.com/model-checking/kani/issues/686 to track a better solution.
                self.symbol_table.insert(sym.clone());
                // As discussed above, fields are named like `0: t1`.
                // Follow that pattern for the marshalled data.
                // name:value map is resilliant to rustc reordering fields (see above)
                (arg_i.to_string().intern(), sym.to_expr())
            }));
        let marshalled_tuple_value =
            Expr::struct_expr(tup_type.clone(), marshalled_tuple_fields, &self.symbol_table)
                .with_location(loc);
        self.declare_variable(
            self.codegen_var_name(&spread_arg),
            self.codegen_var_base_name(&spread_arg),
            tup_type,
            Some(marshalled_tuple_value),
            loc,
        );
    }

    pub fn declare_function(&mut self, instance: Instance) {
        debug!("declaring {}; {:?}", instance.name(), instance);
        let body = self.transformer.body(self.tcx, instance);
        self.set_current_fn(instance, &body);
        debug!(krate=?instance.def.krate(), is_std=self.current_fn().is_std(), "declare_function");
        self.ensure(instance.mangled_name(), |ctx, fname| {
            Symbol::function(
                fname,
                ctx.fn_typ(instance, &body),
                None,
                instance.name(),
                ctx.codegen_span_stable(instance.def.span()),
            )
        });
        self.reset_current_fn();
    }
}

pub mod rustc_smir {
    use crate::codegen_cprover_gotoc::codegen::source_region::{SourceRegion, make_source_region};
    use crate::stable_mir::CrateDef;
    use rustc_middle::mir::coverage::CovTerm;
    use rustc_middle::mir::coverage::MappingKind::Code;
    use rustc_middle::ty::TyCtxt;
    use rustc_smir::rustc_internal;
    use stable_mir::mir::mono::Instance;
    use stable_mir::{Filename, Opaque};

    type CoverageOpaque = stable_mir::Opaque;

    /// Retrieves the `SourceRegion` associated with the data in a
    /// `CoverageOpaque` object.
    pub fn region_from_coverage_opaque(
        tcx: TyCtxt,
        coverage_opaque: &CoverageOpaque,
        instance: Instance,
    ) -> Option<(SourceRegion, Filename)> {
        let cov_term = parse_coverage_opaque(coverage_opaque);
        region_from_coverage(tcx, cov_term, instance)
    }

    /// Retrieves the `SourceRegion` associated with a `CovTerm` object.
    ///
    /// Note: This function could be in the internal `rustc` impl for `Coverage`.
    pub fn region_from_coverage(
        tcx: TyCtxt<'_>,
        coverage: CovTerm,
        instance: Instance,
    ) -> Option<(SourceRegion, Filename)> {
        // We need to pull the coverage info from the internal MIR instance.
        let instance_def = rustc_smir::rustc_internal::internal(tcx, instance.def.def_id());
        let body = tcx.instance_mir(rustc_middle::ty::InstanceKind::Item(instance_def));
        let instance_internal = rustc_smir::rustc_internal::internal(tcx, instance);
        let cov_ids_info = tcx.coverage_ids_info(instance_internal.def).unwrap();
        let bcb_to_covterm = cov_ids_info.term_for_bcb.clone();
        // Some functions, like `std` ones, may not have coverage info attached
        // to them because they have been compiled without coverage flags.
        if let Some(cov_info) = &body.function_coverage_info {
            // Iterate over the coverage mappings and match with the coverage term.
            for mapping in &cov_info.mappings {
                let Code { bcb } = mapping.kind else { unreachable!() };
                let term = match bcb_to_covterm[bcb] {
                    Some(term) => term,
                    _ => CovTerm::Zero,
                };
                let source_map = tcx.sess.source_map();
                let file = source_map.lookup_source_file(cov_info.body_span.lo());
                if term == coverage {
                    return Some((
                        make_source_region(source_map, cov_info, &file, mapping.span).unwrap(),
                        rustc_internal::stable(cov_info.body_span).get_filename(),
                    ));
                }
            }
        }
        None
    }

    /// Parse a `CoverageOpaque` item and return the corresponding `CovTerm`:
    /// <https://doc.rust-lang.org/nightly/nightly-rustc/rustc_middle/mir/coverage/enum.CovTerm.html>
    ///
    /// At present, a `CovTerm` can be one of the following:
    ///  - `CounterIncrement(<num>)`: A physical counter.
    ///  - `ExpressionUsed(<num>)`: An expression-based counter.
    ///  - `Zero`: A counter with a constant zero value.
    fn parse_coverage_opaque(coverage_opaque: &Opaque) -> CovTerm {
        let coverage_str = coverage_opaque.to_string();
        if let Some(rest) = coverage_str.strip_prefix("CounterIncrement(") {
            let (num_str, _rest) = rest.split_once(')').unwrap();
            let num = num_str.parse::<u32>().unwrap();
            CovTerm::Counter(num.into())
        } else if let Some(rest) = coverage_str.strip_prefix("ExpressionUsed(") {
            let (num_str, _rest) = rest.split_once(')').unwrap();
            let num = num_str.parse::<u32>().unwrap();
            CovTerm::Expression(num.into())
        } else {
            CovTerm::Zero
        }
    }
}
