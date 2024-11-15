// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Implementation of the function contracts code generation.
//!
//! The most exciting part is the handling of `requires` and `ensures`, the main
//! entry point to which is [`requires_ensures_main`]. Most of the code
//! generation for that is implemented on [`ContractConditionsHandler`] with
//! [`ContractFunctionState`] steering the code generation. The function state
//! implements a state machine in order to be able to handle multiple attributes
//! on the same function correctly.
//!
//! ## How the handling for `requires`, `modifies`, and `ensures` works.
//!
//! Our aim is to generate a "check" function that can be used to verify the
//! validity of the contract and a "replace" function that can be used as a
//! stub, generated from the contract that can be used instead of the original
//! function.
//!
//! Let me first introduce the constraints which we are operating under to
//! explain why we need the somewhat involved state machine to achieve this.
//!
//! Proc-macros are expanded one-at-a-time, outside-in and they can also be
//! renamed. Meaning the user can do `use kani::requires as precondition` and
//! then use `precondition` everywhere.  We want to support this functionality
//! instead of throwing a hard error but this means we cannot detect if a given
//! function has further contract attributes placed on it during any given
//! expansion. As a result every expansion needs to leave the code in a valid
//! state that could be used for all contract functionality, but it must allow
//! further contract attributes to compose with what was already generated. In
//! addition, we also want to make sure to support non-contract attributes on
//! functions with contracts.
//!
//! To this end we generate attributes in a two-phase approach: initial and subsequent expansions.
//!
//! The initial expansion modifies the original function to contains all necessary instrumentation
//! contracts need to be analyzed. It will do the following:
//! 1. Annotate the function with extra `kanitool` attributes
//! 2. Generate closures for each contract processing scenario (recursive check, simple check,
//! replacement, and regular execution).
//!
//! Subsequent expansions will detect the existence of the extra `kanitool` attributes,
//! and they will only expand the body of the closures generated in the initial phase.
//!
//! Note: We place marker attributes at the bottom of the attribute stack (innermost),
//! otherwise they would not be visible to the future macro expansions.
//!
//! ## Check closure
//!
//! Generates a `__kani_<fn_name>_check` closure that assumes preconditions
//! and asserts postconditions. The check closure is also marked as generated
//! with the `#[kanitool::is_contract_generated(check)]` attribute.
//!
//! Decorates the original function with `#[kanitool::checked_by =
//! "__kani_check_<fn_name>"]`.
//!
//! The check function is a copy of the original function with preconditions
//! added before the body and postconditions after as well as injected before
//! every `return` (see [`PostconditionInjector`]). All arguments are captured
//! by the closure.
//! We also inject contract_cover checks after the precondition and before the postcondition.
//! The precondition cover checks ensure that the precondition is satisfiable; i.e.,
//! that the precondition does not empty the search space and produce a vacuous proof.
//! The postcondition cover checks ensure that the postcondition is reachable.
//!
//! ## Replace Function
//!
//! As the mirror to that also generates a `__kani_replace_<fn_name>`
//! closure that asserts preconditions and assumes postconditions. The replace
//! function is also marked as generated with the
//! `#[kanitool::is_contract_generated(replace)]` attribute.
//!
//! Decorates the original function with `#[kanitool::replaced_by =
//! "__kani_replace_<fn_name>"]`.
//!
//! ## Inductive Verification
//!
//! To efficiently check recursive functions we verify them inductively. To
//! be able to do this we need both the check and replace functions we have seen
//! before.
//!
//! Inductive verification is comprised of a hypothesis and an induction step.
//! The hypothesis in this case is the replace function. It represents the
//! assumption that the contracts holds if the preconditions are satisfied. The
//! induction step is the check function, which ensures that the contract holds,
//! assuming the preconditions hold.
//!
//! Since the induction revolves around the recursive call we can simply set it
//! up upon entry into the body of the function under verification. We use a
//! global variable that tracks whether we are re-entering the function
//! recursively and starts off as `false`. On entry to the function we flip the
//! variable to `true` and dispatch to the check (induction step). If the check
//! recursively calls our function our re-entry tracker now reads `true` and we
//! dispatch to the replacement (application of induction hypothesis). Because
//! the replacement function only checks the conditions and does not perform
//! other computation we will only ever go "one recursion level deep", making
//! inductive verification very efficient. Once the check function returns we
//! flip the tracker variable back to `false` in case the function is called
//! more than once in its harness.
//!
//! To facilitate all this we generate a `__kani_recursion_check_<fn_name>`
//! closure with the following shape:
//!
//! ```ignored
//! let __kani_recursion_check_func = || {
//!     static mut REENTRY: bool = false;
//!
//!     if unsafe { REENTRY } {
//!         let __kani_replace_func = || { /* replace body */ }
//!         __kani_replace_func()
//!     } else {
//!         unsafe { reentry = true };
//!         let __kani_check_func = || { /* check body */ }
//!         let result_kani_internal = __kani_check_func();
//!         unsafe { reentry = false };
//!         result_kani_internal
//!     }
//! };
//! ```
//!
//! We register this closure as `#[kanitool::recursion_check = "__kani_recursion_..."]`.
//!
//! # Complete example
//!
//! ```
//! #[kani::requires(divisor != 0)]
//! #[kani::ensures(|result : &u32| *result <= dividend)]
//! fn div(dividend: u32, divisor: u32) -> u32 {
//!     dividend / divisor
//! }
//! ```
//!
//! Turns into
//! ```
//! #[kanitool::recursion_check = "__kani_recursion_check_div"]
//! #[kanitool::checked_with = "__kani_check_div"]
//! #[kanitool::replaced_with = "__kani_replace_div"]
//! #[kanitool::modifies_wrapper = "__kani_modifies_div"]
//! fn div(dividend: u32, divisor: u32) -> u32 {
//!     #[inline(never)]
//!     #[kanitool::fn_marker = "kani_register_contract"]
//!     pub const fn kani_register_contract<T, F: FnOnce() -> T>(f: F) -> T {
//!         kani::panic("internal error: entered unreachable code: ")
//!     }
//!     let kani_contract_mode = kani::internal::mode();
//!     match kani_contract_mode {
//!         kani::internal::RECURSION_CHECK => {
//!             #[kanitool::is_contract_generated(recursion_check)]
//!             #[allow(dead_code, unused_variables, unused_mut)]
//!             let mut __kani_recursion_check_div =
//!                 || -> u32
//!                     {
//!                         #[kanitool::recursion_tracker]
//!                         static mut REENTRY: bool = false;
//!                         if unsafe { REENTRY } {
//!                                 #[kanitool::is_contract_generated(replace)]
//!                                 #[allow(dead_code, unused_variables, unused_mut)]
//!                                 let mut __kani_replace_div =
//!                                     || -> u32
//!                                         {
//!                                             kani::assert(divisor != 0, "divisor != 0");
//!                                             let result_kani_internal: u32 = kani::any_modifies();
//!                                             kani::assume(kani::internal::apply_closure(|result: &u32|
//!                                                         *result <= dividend, &result_kani_internal));
//!                                             result_kani_internal
//!                                         };
//!                                 __kani_replace_div()
//!                             } else {
//!                                unsafe { REENTRY = true };
//!                                #[kanitool::is_contract_generated(check)]
//!                                #[allow(dead_code, unused_variables, unused_mut)]
//!                                let mut __kani_check_div =
//!                                    || -> u32
//!                                        {
//!                                            kani::assume(divisor != 0);
//!                                            kani::internal::contract_cover(divisor != 0, "The contract's precondition is satisfiable.");
//!                                            let _wrapper_arg = ();
//!                                            #[kanitool::is_contract_generated(wrapper)]
//!                                            #[allow(dead_code, unused_variables, unused_mut)]
//!                                            let mut __kani_modifies_div =
//!                                                |_wrapper_arg| -> u32 { dividend / divisor };
//!                                            let result_kani_internal: u32 =
//!                                                __kani_modifies_div(_wrapper_arg);
//!                                            kani::internal::contract_cover(
//!                                                 kani::internal::apply_closure(|result: &u32|
//!                                                     *result <= dividend,
//!                                                     &result_kani_internal),
//!                                                 "The contract's postcondition is reachable.");
//!                                            kani::assert(kani::internal::apply_closure(|result: &u32|
//!                                                        *result <= dividend, &result_kani_internal),
//!                                                "|result : &u32| *result <= dividend");
//!                                            result_kani_internal
//!                                        };
//!                                let result_kani_internal = __kani_check_div();
//!                                unsafe { REENTRY = false };
//!                                result_kani_internal
//!                            }
//!                     };
//!             ;
//!             kani_register_contract(__kani_recursion_check_div)
//!         }
//!         kani::internal::REPLACE => {
//!             #[kanitool::is_contract_generated(replace)]
//!             #[allow(dead_code, unused_variables, unused_mut)]
//!             let mut __kani_replace_div =
//!                 || -> u32
//!                     {
//!                         kani::assert(divisor != 0, "divisor != 0");
//!                         let result_kani_internal: u32 = kani::any_modifies();
//!                         kani::assume(kani::internal::apply_closure(|result: &u32|
//!                                     *result <= dividend, &result_kani_internal));
//!                         result_kani_internal
//!                     };
//!             ;
//!             kani_register_contract(__kani_replace_div)
//!         }
//!         kani::internal::SIMPLE_CHECK => {
//!             #[kanitool::is_contract_generated(check)]
//!             #[allow(dead_code, unused_variables, unused_mut)]
//!             let mut __kani_check_div =
//!                 || -> u32
//!                     {
//!                         kani::assume(divisor != 0);
//!                         kani::internal::contract_cover(divisor != 0, "The contract's precondition is satisfiable.");
//!                         let _wrapper_arg = ();
//!                         #[kanitool::is_contract_generated(wrapper)]
//!                         #[allow(dead_code, unused_variables, unused_mut)]
//!                         let mut __kani_modifies_div =
//!                             |_wrapper_arg| -> u32 { dividend / divisor };
//!                         let result_kani_internal: u32 =
//!                             __kani_modifies_div(_wrapper_arg);
//!                         kani::internal::contract_cover(
//!                             kani::internal::apply_closure(|result: &u32|
//!                                 *result <= dividend,
//!                                 &result_kani_internal),
//!                             "The contract's postcondition is reachable.");
//!                         kani::assert(kani::internal::apply_closure(|result: &u32|
//!                                     *result <= dividend, &result_kani_internal),
//!                             "|result : &u32| *result <= dividend");
//!                         result_kani_internal
//!                     };
//!             ;
//!             kani_register_contract(__kani_check_div)
//!         }
//!         _ => { dividend / divisor }
//!     }
//! }
//! ```
//!
//! Additionally, there is functionality that allows the referencing of
//! history values within the ensures statement. This means we can
//! precompute a value before the function is called and have access to
//! this value in the later ensures statement. This is done via the
//! `old` monad which lets you access the old state within the present
//! state. Each occurrence of `old` is lifted, so is is necessary that
//! each lifted occurrence is closed with respect to the function arguments.
//! The results of these old computations are placed into
//! `remember_kani_internal_XXX` variables which are hashed. Consider the following example:
//!
//! ```
//! #[kani::ensures(|result| old(*ptr + 1) == *ptr)]
//! #[kani::ensures(|result| old(*ptr + 1) == *ptr)]
//! #[kani::requires(*ptr < 100)]
//! #[kani::modifies(ptr)]
//! fn modify(ptr: &mut u32) {
//!     *ptr += 1;
//! }
//!
//! #[kani::proof_for_contract(modify)]
//! fn main() {
//!     let mut i = kani::any();
//!     modify(&mut i);
//! }
//!
//! ```
//!
//! This expands to
//!
//! ```
//! #[kanitool::recursion_check = "__kani_recursion_check_modify"]
//! #[kanitool::checked_with = "__kani_check_modify"]
//! #[kanitool::replaced_with = "__kani_replace_modify"]
//! #[kanitool::modifies_wrapper = "__kani_modifies_modify"]
//! fn modify(ptr: &mut u32) {
//!     #[inline(never)]
//!     #[kanitool::fn_marker = "kani_register_contract"]
//!     pub const fn kani_register_contract<T, F: FnOnce() -> T>(f: F) -> T {
//!         kani::panic("internal error: entered unreachable code: ")
//!     }
//!     let kani_contract_mode = kani::internal::mode();
//!     match kani_contract_mode {
//!         kani::internal::RECURSION_CHECK => {
//!             #[kanitool::is_contract_generated(recursion_check)]
//!             #[allow(dead_code, unused_variables, unused_mut)]
//!             let mut __kani_recursion_check_modify =
//!                 ||
//!                     {
//!                         #[kanitool::recursion_tracker]
//!                         static mut REENTRY: bool = false;
//!                         if unsafe { REENTRY } {
//!                                 #[kanitool::is_contract_generated(replace)]
//!                                 #[allow(dead_code, unused_variables, unused_mut)]
//!                                 let mut __kani_replace_modify =
//!                                     ||
//!                                         {
//!                                             kani::assert(*ptr < 100, "*ptr < 100");
//!                                             let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                                             let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                                             let result_kani_internal: () = kani::any_modifies();
//!                                             *unsafe {
//!                                                         kani::internal::Pointer::assignable(kani::internal::untracked_deref(&(ptr)))
//!                                                     } = kani::any_modifies();
//!                                             kani::assume(kani::internal::apply_closure(|result|
//!                                                         (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                                     &result_kani_internal));
//!                                             kani::assume(kani::internal::apply_closure(|result|
//!                                                         (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                                     &result_kani_internal));
//!                                             result_kani_internal
//!                                         };
//!                                 __kani_replace_modify()
//!                             } else {
//!                                unsafe { REENTRY = true };
//!                                #[kanitool::is_contract_generated(check)]
//!                                #[allow(dead_code, unused_variables, unused_mut)]
//!                                let mut __kani_check_modify =
//!                                    ||
//!                                        {
//!                                            kani::assume(*ptr < 100);
//!                                            kani::internal::contract_cover(*ptr < 100, "The contract's precondition is satisfiable.");
//!                                            let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                                            let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                                            let _wrapper_arg = (ptr as *const _,);
//!                                            #[kanitool::is_contract_generated(wrapper)]
//!                                            #[allow(dead_code, unused_variables, unused_mut)]
//!                                            let mut __kani_modifies_modify =
//!                                                |_wrapper_arg| { *ptr += 1; };
//!                                            let result_kani_internal: () =
//!                                                __kani_modifies_modify(_wrapper_arg);
//!                                            kani::internal::contract_cover(
//!                                                kani::internal::apply_closure(|result|
//!                                                    (remember_kani_internal_2e780b148d45b5c8) == * ptr,
//!                                                    &result_kani_internal
//!                                                ),
//!                                            "The contract's postcondition is reachable.");
//!                                            kani::assert(kani::internal::apply_closure(|result|
//!                                                        (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                                    &result_kani_internal), "|result| old(*ptr + 1) == *ptr");
//!                                            kani::internal::contract_cover(
//!                                                kani::internal::apply_closure(|result|
//!                                                    (remember_kani_internal_2e780b148d45b5c8) == * ptr,
//!                                                    &result_kani_internal
//!                                                ),
//!                                            "The contract's postcondition is reachable.");
//!                                            kani::assert(kani::internal::apply_closure(|result|
//!                                                        (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                                    &result_kani_internal), "|result| old(*ptr + 1) == *ptr");
//!                                            result_kani_internal
//!                                        };
//!                                let result_kani_internal = __kani_check_modify();
//!                                unsafe { REENTRY = false };
//!                                result_kani_internal
//!                            }
//!                     };
//!             ;
//!             kani_register_contract(__kani_recursion_check_modify)
//!         }
//!         kani::internal::REPLACE => {
//!             #[kanitool::is_contract_generated(replace)]
//!             #[allow(dead_code, unused_variables, unused_mut)]
//!             let mut __kani_replace_modify =
//!                 ||
//!                     {
//!                         kani::assert(*ptr < 100, "*ptr < 100");
//!                         let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                         let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                         let result_kani_internal: () = kani::any_modifies();
//!                         *unsafe {
//!                                     kani::internal::Pointer::assignable(kani::internal::untracked_deref(&(ptr)))
//!                                 } = kani::any_modifies();
//!                         kani::assume(kani::internal::apply_closure(|result|
//!                                     (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                 &result_kani_internal));
//!                         kani::assume(kani::internal::apply_closure(|result|
//!                                     (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                 &result_kani_internal));
//!                         result_kani_internal
//!                     };
//!             ;
//!             kani_register_contract(__kani_replace_modify)
//!         }
//!         kani::internal::SIMPLE_CHECK => {
//!             #[kanitool::is_contract_generated(check)]
//!             #[allow(dead_code, unused_variables, unused_mut)]
//!             let mut __kani_check_modify =
//!                 ||
//!                     {
//!                         kani::assume(*ptr < 100);
//!                         kani::internal::contract_cover(*ptr < 100, "The contract's precondition is satisfiable.");
//!                         let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                         let remember_kani_internal_92cc419d8aca576c = *ptr + 1;
//!                         let _wrapper_arg = (ptr as *const _,);
//!                         #[kanitool::is_contract_generated(wrapper)]
//!                         #[allow(dead_code, unused_variables, unused_mut)]
//!                         let mut __kani_modifies_modify =
//!                             |_wrapper_arg| { *ptr += 1; };
//!                         let result_kani_internal: () =
//!                             __kani_modifies_modify(_wrapper_arg);
//!                         kani::internal::contract_cover(
//!                            kani::internal::apply_closure(|result|
//!                              (remember_kani_internal_2e780b148d45b5c8) == * ptr,
//!                              &result_kani_internal
//!                             ),
//!                         "The contract's postcondition is reachable.");
//!                         kani::assert(kani::internal::apply_closure(|result|
//!                                     (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                 &result_kani_internal), "|result| old(*ptr + 1) == *ptr");
//!                         kani::internal::contract_cover(
//!                            kani::internal::apply_closure(|result|
//!                              (remember_kani_internal_2e780b148d45b5c8) == * ptr,
//!                              &result_kani_internal
//!                             ),
//!                         "The contract's postcondition is reachable.");
//!                         kani::assert(kani::internal::apply_closure(|result|
//!                                     (remember_kani_internal_92cc419d8aca576c) == *ptr,
//!                                 &result_kani_internal), "|result| old(*ptr + 1) == *ptr");
//!                         result_kani_internal
//!                     };
//!             ;
//!             kani_register_contract(__kani_check_modify)
//!         }
//!         _ => { *ptr += 1; }
//!     }
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro2::{Ident, TokenStream as TokenStream2};
use quote::quote;
use syn::{Expr, ExprClosure, ItemFn, parse_macro_input, parse_quote};

mod bootstrap;
mod check;
#[macro_use]
mod helpers;
mod initialize;
mod replace;
mod shared;

const INTERNAL_RESULT_IDENT: &str = "result_kani_internal";

pub fn requires(attr: TokenStream, item: TokenStream) -> TokenStream {
    contract_main(attr, item, ContractConditionsType::Requires)
}

pub fn ensures(attr: TokenStream, item: TokenStream) -> TokenStream {
    contract_main(attr, item, ContractConditionsType::Ensures)
}

pub fn modifies(attr: TokenStream, item: TokenStream) -> TokenStream {
    contract_main(attr, item, ContractConditionsType::Modifies)
}

/// This is very similar to the kani_attribute macro, but it instead creates
/// key-value style attributes which I find a little easier to parse.
macro_rules! passthrough {
    ($name:ident, $allow_dead_code:ident) => {
        pub fn $name(attr: TokenStream, item: TokenStream) -> TokenStream {
            let args = proc_macro2::TokenStream::from(attr);
            let fn_item = proc_macro2::TokenStream::from(item);
            let name = Ident::new(stringify!($name), proc_macro2::Span::call_site());
            let extra_attrs = if $allow_dead_code {
                quote!(#[allow(dead_code)])
            } else {
                quote!()
            };
            quote!(
                #extra_attrs
                #[kanitool::#name = stringify!(#args)]
                #fn_item
            )
            .into()
        }
    }
}

passthrough!(stub_verified, false);

pub fn proof_for_contract(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = proc_macro2::TokenStream::from(attr);
    let mut fn_item = parse_macro_input!(item as ItemFn);
    fn_item.block.stmts.insert(0, parse_quote!(kani::internal::init_contracts();));
    quote!(
        #[allow(dead_code)]
        #[kanitool::proof_for_contract = stringify!(#args)]
        #fn_item
    )
    .into()
}

/// Classifies the state a function is in the contract handling pipeline.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ContractFunctionState {
    /// This is the function already expanded with the closures.
    Expanded,
    /// This is the first time a contract attribute is evaluated on this
    /// function.
    Untouched,
}

/// The information needed to generate the bodies of check and replacement
/// functions that integrate the conditions from this contract attribute.
struct ContractConditionsHandler<'a> {
    /// Information specific to the type of contract attribute we're expanding.
    condition_type: ContractConditionsData,
    /// Body of the function this attribute was found on.
    annotated_fn: &'a ItemFn,
    /// An unparsed, unmodified copy of `attr`, used in the error messages.
    attr_copy: TokenStream2,
    /// The stream to which we should write the generated code.
    output: TokenStream2,
    /// The name of the check closure.
    check_name: String,
    /// The name of the replace closure.
    replace_name: String,
    /// The name of the recursion closure.
    recursion_name: String,
    /// The name of the modifies closure.
    modify_name: String,
}

/// Which kind of contract attribute are we dealing with?
///
/// Pre-parsing version of [`ContractConditionsData`].
#[derive(Copy, Clone, Eq, PartialEq)]
enum ContractConditionsType {
    Requires,
    Ensures,
    Modifies,
}

/// Clause-specific information mostly generated by parsing the attribute.
///
/// [`ContractConditionsType`] is the corresponding pre-parse version.
enum ContractConditionsData {
    Requires {
        /// The contents of the attribute.
        attr: Expr,
    },
    Ensures {
        /// The contents of the attribute.
        attr: ExprClosure,
    },
    Modifies {
        attr: Vec<Expr>,
    },
}

/// Which function are we currently generating?
#[derive(Copy, Clone, Eq, PartialEq)]
enum ClosureType {
    Check,
    Replace,
}

impl<'a> ContractConditionsHandler<'a> {
    /// Handle the contract state and return the generated code
    fn dispatch_on(mut self, state: ContractFunctionState) -> TokenStream2 {
        match state {
            // We are on the already expanded function.
            ContractFunctionState::Expanded => self.handle_expanded(),
            ContractFunctionState::Untouched => self.handle_untouched(),
        }
        self.output
    }
}

/// The main meat of handling requires/ensures contracts.
///
/// See the [module level documentation][self] for a description of how the code
/// generation works.
fn contract_main(
    attr: TokenStream,
    item: TokenStream,
    is_requires: ContractConditionsType,
) -> TokenStream {
    let attr_copy = TokenStream2::from(attr.clone());
    let mut item_fn = parse_macro_input!(item as ItemFn);
    let function_state = ContractFunctionState::from_attributes(&item_fn.attrs);
    let handler = match ContractConditionsHandler::new(is_requires, attr, &mut item_fn, attr_copy) {
        Ok(handler) => handler,
        Err(e) => return e.into_compile_error().into(),
    };

    handler.dispatch_on(function_state).into()
}
