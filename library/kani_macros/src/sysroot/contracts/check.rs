// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Logic used for generating the code that checks a contract.

use proc_macro2::{Ident, Span, TokenStream as TokenStream2};
use quote::quote;
use std::mem;
use syn::{Block, Expr, FnArg, Local, LocalInit, Pat, PatIdent, ReturnType, Stmt, parse_quote};

use super::{
    ClosureType, ContractConditionsData, ContractConditionsHandler, INTERNAL_RESULT_IDENT,
    helpers::*, shared::build_ensures,
};

const WRAPPER_ARG: &str = "_wrapper_arg";

impl<'a> ContractConditionsHandler<'a> {
    /// Create the body of a check function.
    ///
    /// Wraps the conditions from this attribute around `self.body`.
    pub fn make_check_body(&self, mut body_stmts: Vec<Stmt>) -> TokenStream2 {
        let Self { attr_copy, .. } = self;
        match &self.condition_type {
            ContractConditionsData::Requires { attr } => {
                quote!({
                    kani::assume(#attr);
                    kani::internal::contract_cover(#attr, "The contract's precondition is satisfiable.");
                    #(#body_stmts)*
                })
            }
            ContractConditionsData::Ensures { attr } => {
                let (remembers, ensures_clause) = build_ensures(attr);

                // The code that enforces the postconditions and cleans up the shallow
                // argument copies (with `mem::forget`).
                let exec_postconditions = quote!(
                    kani::assert(#ensures_clause, stringify!(#attr_copy));
                );

                let return_expr = body_stmts.pop();

                let (assumes, rest_of_body) =
                    split_for_remembers(&body_stmts[..], ClosureType::Check);

                quote!({
                    #(#assumes)*
                    #remembers
                    #(#rest_of_body)*
                    kani::internal::contract_cover(#ensures_clause, "The contract's postcondition is reachable.");
                    #exec_postconditions
                    #return_expr
                })
            }
            ContractConditionsData::Modifies { attr } => {
                let wrapper_arg_ident = Ident::new(WRAPPER_ARG, Span::call_site());
                let wrapper_tuple = body_stmts.iter_mut().find_map(|stmt| {
                    if let Stmt::Local(Local {
                        pat: Pat::Ident(PatIdent { ident, .. }),
                        init: Some(LocalInit { expr, .. }),
                        ..
                    }) = stmt
                    {
                        (ident == &wrapper_arg_ident).then_some(expr.as_mut())
                    } else {
                        None
                    }
                });
                if let Some(Expr::Tuple(values)) = wrapper_tuple {
                    values.elems.extend(attr.iter().map(|attr| {
                        let expr: Expr = parse_quote!(#attr
                        as *const _);
                        expr
                    }));
                } else {
                    unreachable!("Expected tuple but found `{wrapper_tuple:?}`")
                }
                quote!({#(#body_stmts)*})
            }
        }
    }

    /// Initialize the list of statements for the check closure body.
    fn initial_check_stmts(&self) -> Vec<syn::Stmt> {
        let modifies_ident = Ident::new(&self.modify_name, Span::call_site());
        let wrapper_arg_ident = Ident::new(WRAPPER_ARG, Span::call_site());
        let return_type = return_type_to_type(&self.annotated_fn.sig.output);
        let mut_recv = self.has_mutable_receiver().then(|| quote!(core::ptr::addr_of!(self),));
        let redefs = self.arg_redefinitions();
        let modifies_closure =
            self.modifies_closure(&self.annotated_fn.sig.output, &self.annotated_fn.block, redefs);
        let result = Ident::new(INTERNAL_RESULT_IDENT, Span::call_site());
        parse_quote!(
            let #wrapper_arg_ident = (#mut_recv);
            #modifies_closure
            let #result : #return_type = #modifies_ident(#wrapper_arg_ident);
            #result
        )
    }

    /// Generate a token stream that represents the check closure.
    ///
    /// See [`Self::make_check_body`] for the most interesting parts of this
    /// function.
    pub fn check_closure(&self) -> TokenStream2 {
        let check_ident = Ident::new(&self.check_name, Span::call_site());
        let sig = &self.annotated_fn.sig;
        let output = &sig.output;
        let body_stmts = self.initial_check_stmts();
        let body = self.make_check_body(body_stmts);

        quote!(
            #[kanitool::is_contract_generated(check)]
            #[allow(dead_code, unused_variables, unused_mut)]
            let mut #check_ident = || #output #body;
        )
    }

    /// Expand the check body.
    ///
    /// First find the modifies body and expand that. Then expand the rest of the body.
    pub fn expand_check(&self, closure: &mut Stmt) {
        let body = closure_body(closure);
        self.expand_modifies(find_contract_closure(&mut body.block.stmts, "wrapper").expect(
            &format!("Internal Failure: Expected to find `wrapper` closure, but found none"),
        ));
        *body = syn::parse2(self.make_check_body(mem::take(&mut body.block.stmts))).unwrap();
    }

    /// Emit a modifies wrapper. It's only argument is the list of addresses that may be modified.
    pub fn modifies_closure(
        &self,
        output: &ReturnType,
        body: &Block,
        redefs: TokenStream2,
    ) -> TokenStream2 {
        // Filter receiver
        let wrapper_ident = Ident::new(WRAPPER_ARG, Span::call_site());
        let modifies_ident = Ident::new(&self.modify_name, Span::call_site());
        let stmts = &body.stmts;
        quote!(
            #[kanitool::is_contract_generated(wrapper)]
            #[allow(dead_code, unused_variables, unused_mut)]
            let mut #modifies_ident = |#wrapper_ident: _| #output {
                #redefs
                #(#stmts)*
            };
        )
    }

    /// Expand the modifies closure if we are handling a modifies attribute. Otherwise, no-op.
    pub fn expand_modifies(&self, closure_stmt: &mut Stmt) {
        if matches!(&self.condition_type, ContractConditionsData::Modifies { .. }) {
            let Stmt::Local(Local { init: Some(LocalInit { expr, .. }), .. }) = closure_stmt else {
                unreachable!()
            };
            let Expr::Closure(closure) = expr.as_ref() else { unreachable!() };
            let Expr::Block(body) = closure.body.as_ref() else { unreachable!() };
            let stream = self.modifies_closure(&closure.output, &body.block, TokenStream2::new());
            *closure_stmt = syn::parse2(stream).unwrap();
        }
    }

    /// Return whether the original function has a mutable receiver.
    fn has_mutable_receiver(&self) -> bool {
        let first_arg = self.annotated_fn.sig.inputs.first();
        first_arg
            .map(|arg| {
                matches!(
                    arg,
                    FnArg::Receiver(syn::Receiver { mutability: Some(_), reference: None, .. },)
                )
            })
            .unwrap_or(false)
    }

    /// Generate argument re-definitions for mutable arguments.
    ///
    /// This is used so Kani doesn't think that modifying a local argument value is a side effect.
    fn arg_redefinitions(&self) -> TokenStream2 {
        let mut result = TokenStream2::new();
        for (mutability, ident) in self.arg_bindings() {
            if mutability == MutBinding::Mut {
                result.extend(quote!(let mut #ident = #ident;))
            } else {
                // This would make some replace some temporary variables from error messages.
                //result.extend(quote!(let #ident = #ident; ))
            }
        }
        result
    }

    /// Extract all arguments bindings and their mutability.
    fn arg_bindings(&self) -> impl Iterator<Item = (MutBinding, &Ident)> {
        self.annotated_fn.sig.inputs.iter().flat_map(|arg| match arg {
            FnArg::Receiver(_) => vec![],
            FnArg::Typed(typed) => pat_to_bindings(typed.pat.as_ref()),
        })
    }
}
