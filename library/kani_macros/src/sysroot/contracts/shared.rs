// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Logic that is shared between [`super::initialize`], [`super::check`] and
//! [`super::replace`].
//!
//! This is so we can keep [`super`] distraction-free as the definitions of data
//! structures and the entry point for contract handling.

use std::collections::HashMap;

use proc_macro2::{Ident, Span, TokenStream as TokenStream2, TokenStream};
use quote::{quote, ToTokens};
use std::hash::{DefaultHasher, Hash, Hasher};
use syn::{
    spanned::Spanned, visit_mut::VisitMut, Attribute, Expr, ExprCall, ExprClosure, ExprPath, Path,
};

use super::{ContractConditionsHandler, ContractFunctionState, INTERNAL_RESULT_IDENT};

impl ContractFunctionState {
    /// Do we need to emit the `is_contract_generated` tag attribute on the
    /// generated function(s)?
    pub fn emit_tag_attr(self) -> bool {
        matches!(self, ContractFunctionState::Untouched)
    }
}

impl<'a> ContractConditionsHandler<'a> {
    pub fn is_first_emit(&self) -> bool {
        matches!(self.function_state, ContractFunctionState::Untouched)
    }
}

/// Used as the "single source of truth" for [`try_as_result_assign`] and [`try_as_result_assign_mut`]
/// since we can't abstract over mutability. Input is the object to match on and the name of the
/// function used to convert an `Option<LocalInit>` into the result type (e.g. `as_ref` and `as_mut`
/// respectively).
///
/// We start with a `match` as a top-level here, since if we made this a pattern macro (the "clean"
/// thing to do) then we cant use the `if` inside there which we need because box patterns are
/// unstable.
macro_rules! try_as_result_assign_pat {
    ($input:expr, $convert:ident) => {
        match $input {
            syn::Stmt::Local(syn::Local {
                pat: syn::Pat::Type(syn::PatType {
                    pat: inner_pat,
                    attrs,
                    ..
                }),
                init,
                ..
            }) if attrs.is_empty()
            && matches!(
                inner_pat.as_ref(),
                syn::Pat::Ident(syn::PatIdent {
                    by_ref: None,
                    mutability: None,
                    ident: result_ident,
                    subpat: None,
                    ..
                }) if result_ident == INTERNAL_RESULT_IDENT
            ) => init.$convert(),
            _ => None,
        }
    };
}

/// Try to parse this statement as `let result : <...> = <init>;` and return `init`.
///
/// This is the shape of statement we create in replace functions to havoc (with `init` being
/// `kani::any()`) and we need to recognize it for when we edit the replace function and integrate
/// additional conditions.
///
/// It's a thin wrapper around [`try_as_result_assign_pat!`] to create an immutable match.
pub fn try_as_result_assign(stmt: &syn::Stmt) -> Option<&syn::LocalInit> {
    try_as_result_assign_pat!(stmt, as_ref)
}

/// Try to parse this statement as `let result : <...> = <init>;` and return a mutable reference to
/// `init`.
///
/// This is the shape of statement we create in check functions (with `init` being a call to check
/// function with additional pointer arguments for the `modifies` clause) and we need to recognize
/// it to then edit this call if we find another `modifies` clause and add its additional arguments.
/// additional conditions.
///
/// It's a thin wrapper around [`try_as_result_assign_pat!`] to create a mutable match.
pub fn try_as_result_assign_mut(stmt: &mut syn::Stmt) -> Option<&mut syn::LocalInit> {
    try_as_result_assign_pat!(stmt, as_mut)
}

/// When a `#[kani::ensures(|result|expr)]` is expanded, this function is called on with `build_ensures(|result|expr)`.
/// This function goes through the expr and extracts out all the `old` expressions and creates a sequence
/// of statements that instantiate these expressions as `let remember_kani_internal_x = old_expr;`
/// where x is a unique hash. This is returned as the first return parameter. The second
/// return parameter is the expression formed by passing in the result variable into the input closure.
pub fn build_ensures(data: &ExprClosure) -> (TokenStream2, Expr) {
    let mut remembers_exprs = HashMap::new();
    let mut vis = OldVisitor { t: OldLifter::new(), remembers_exprs: &mut remembers_exprs };
    let mut expr = &mut data.clone();
    vis.visit_expr_closure_mut(&mut expr);

    let remembers_stmts: TokenStream2 = remembers_exprs
        .iter()
        .fold(quote!(), |collect, (ident, expr)| quote!(let #ident = #expr; #collect));

    let result: Ident = Ident::new(INTERNAL_RESULT_IDENT, Span::call_site());
    (remembers_stmts, Expr::Verbatim(quote!(kani::internal::apply_closure(#expr, &#result))))
}

trait OldTrigger {
    /// You are provided with the expression that is the first argument of the
    /// `old()` call. You may modify it as you see fit. The return value
    /// indicates whether the entire `old()` call should be replaced by the
    /// (potentially altered) first argument.
    ///
    /// The second argument is the span of the original `old` expression.
    ///
    /// The third argument is a collection of all the expressions that need to be lifted
    /// into the past environment as new remember variables.
    fn trigger(&mut self, e: &mut Expr, s: Span, output: &mut HashMap<Ident, Expr>) -> bool;
}

struct OldLifter;

impl OldLifter {
    fn new() -> Self {
        Self
    }
}

struct OldDenier;

impl OldTrigger for OldDenier {
    fn trigger(&mut self, _: &mut Expr, s: Span, _: &mut HashMap<Ident, Expr>) -> bool {
        s.unwrap().error("Nested calls to `old` are prohibited").emit();
        false
    }
}

struct OldVisitor<'a, T> {
    t: T,
    remembers_exprs: &'a mut HashMap<Ident, Expr>,
}

impl<T: OldTrigger> syn::visit_mut::VisitMut for OldVisitor<'_, T> {
    fn visit_expr_mut(&mut self, ex: &mut Expr) {
        let trigger = match &*ex {
            Expr::Call(call @ ExprCall { func, attrs, args, .. }) => match func.as_ref() {
                Expr::Path(ExprPath {
                    attrs: func_attrs,
                    qself: None,
                    path: Path { leading_colon: None, segments },
                }) if segments.len() == 1
                    && segments.first().map_or(false, |sgm| sgm.ident == "old") =>
                {
                    let first_segment = segments.first().unwrap();
                    assert_spanned_err!(first_segment.arguments.is_empty(), first_segment);
                    assert_spanned_err!(attrs.is_empty(), call);
                    assert_spanned_err!(func_attrs.is_empty(), func);
                    assert_spanned_err!(args.len() == 1, call);
                    true
                }
                _ => false,
            },
            _ => false,
        };
        if trigger {
            let span = ex.span();
            let new_expr = if let Expr::Call(ExprCall { ref mut args, .. }) = ex {
                self.t
                    .trigger(args.iter_mut().next().unwrap(), span, self.remembers_exprs)
                    .then(|| args.pop().unwrap().into_value())
            } else {
                unreachable!()
            };
            if let Some(new) = new_expr {
                let _ = std::mem::replace(ex, new);
            }
        } else {
            syn::visit_mut::visit_expr_mut(self, ex)
        }
    }
}

impl OldTrigger for OldLifter {
    fn trigger(
        &mut self,
        e: &mut Expr,
        _: Span,
        remembers_exprs: &mut HashMap<Ident, Expr>,
    ) -> bool {
        let mut denier = OldVisitor { t: OldDenier, remembers_exprs };
        // This ensures there are no nested calls to `old`
        denier.visit_expr_mut(e);
        let mut hasher = DefaultHasher::new();
        e.hash(&mut hasher);
        let ident =
            Ident::new(&format!("remember_kani_internal_{:x}", hasher.finish()), Span::call_site());
        // save the original expression to be lifted into the past remember environment
        remembers_exprs.insert(ident.clone(), (*e).clone());
        // change the expression to refer to the new remember variable
        let _ = std::mem::replace(e, Expr::Verbatim(quote!((#ident))));
        true
    }
}
