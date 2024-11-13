// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT
//! This module contains code for handling special functions from the Kani library.
//!
//! There are three types of functions today:
//!    1. Kani intrinsics: These are functions whose body is generated during
//!       compilation time. Their body usually require some extra knowledge about the given types
//!       that's only available during compilation.
//!    2. Kani models: These are functions that model a specific behavior but that cannot be used
//!       directly by the user. For example, retrieving information about memory initialization.
//!       Kani compiler determines when and where to use these models, but the logic itself is
//!       encoded in Rust.
//!    3. Kani hooks: These are similar to Kani intrinsics but their code generation depends
//!       on some backend specific logic. From a Kani library perspective, there is no difference
//!       between hooks and intrinsics. This is a compiler implementation detail.
//!
//! Functions #1 and #2 have a `kanitool::fn_marker` attribute attached to them.
//! The marker value will contain "Intrinsic" or "Model" suffix, indicating which category they
//! fit in.
//!
//! We are transitioning the third category. It mostly uses rustc diagnostics. See how they are
//! handled in [crate::codegen_cprover_gotoc::overrides::hooks].
//!
//! Note that we still need to migrate some of 1 and 2 to this new structure which are currently
//! using rustc's diagnostic infrastructure.

use crate::kani_middle::attributes;
use stable_mir::mir::mono::Instance;
use stable_mir::ty::FnDef;
use std::collections::HashMap;
use std::str::FromStr;
use strum_macros::{EnumString, IntoStaticStr};
use tracing::{debug, trace};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum KaniFunction {
    Model(KaniModel),
    Intrinsic(KaniIntrinsic),
}

/// Kani intrinsics are functions generated by the compiler.
///
/// These functions usually depend on information that require extra knowledge about the type
/// or extra Kani instrumentation.
#[derive(Debug, Copy, Clone, Eq, PartialEq, IntoStaticStr, EnumString, Hash)]
pub enum KaniIntrinsic {
    #[strum(serialize = "ValidValueIntrinsic")]
    ValidValue,
    #[strum(serialize = "IsInitializedIntrinsic")]
    IsInitialized,
    #[strum(serialize = "CheckedAlignOfIntrinsic")]
    CheckedAlignOf,
    #[strum(serialize = "CheckedSizeOfIntrinsic")]
    CheckedSizeOf,
    #[strum(serialize = "SafetyCheckIntrinsic")]
    SafetyCheck,
}

/// Kani models are Rust functions that model some runtime behavior used by Kani instrumentation.
#[derive(Debug, Copy, Clone, Eq, PartialEq, IntoStaticStr, EnumString, Hash)]
pub enum KaniModel {
    #[strum(serialize = "IsStrPtrInitializedModel")]
    IsStrPtrInitialized,
    #[strum(serialize = "IsSlicePtrInitializedModel")]
    IsSlicePtrInitialized,
    #[strum(serialize = "SizeOfValRawModel")]
    SizeOfVal,
    #[strum(serialize = "AlignOfValRawModel")]
    AlignOfVal,
    #[strum(serialize = "SizeOfDynObjectModel")]
    SizeOfDynObject,
    #[strum(serialize = "AlignOfDynObjectModel")]
    AlignOfDynObject,
    #[strum(serialize = "SizeOfSliceObjectModel")]
    SizeOfSliceObject,
}

impl From<KaniIntrinsic> for KaniFunction {
    fn from(value: KaniIntrinsic) -> Self {
        KaniFunction::Intrinsic(value)
    }
}

impl From<KaniModel> for KaniFunction {
    fn from(value: KaniModel) -> Self {
        KaniFunction::Model(value)
    }
}

impl TryFrom<FnDef> for KaniFunction {
    type Error = ();

    fn try_from(def: FnDef) -> Result<Self, Self::Error> {
        let value = attributes::fn_marker(def).ok_or(())?;
        if let Ok(intrisic) = KaniIntrinsic::from_str(&value) {
            Ok(intrisic.into())
        } else if let Ok(model) = KaniModel::from_str(&value) {
            Ok(model.into())
        } else {
            Err(())
        }
    }
}

impl TryFrom<Instance> for KaniFunction {
    type Error = ();

    fn try_from(instance: Instance) -> Result<Self, Self::Error> {
        let value = attributes::fn_marker(instance.def).ok_or(())?;
        if let Ok(intrisic) = KaniIntrinsic::from_str(&value) {
            Ok(intrisic.into())
        } else if let Ok(model) = KaniModel::from_str(&value) {
            Ok(model.into())
        } else {
            Err(())
        }
    }
}

/// Find all Kani functions.
///
/// First try to find `kani` crate. If that exists, look for the items there.
/// If there's no Kani crate, look for the items in `core` since we could be using `kani_core`.
/// Note that users could have other `kani` crates, so we look in all the ones we find.
///
/// TODO: We should check if there is no name conflict and that we found all functions.
pub fn find_kani_functions() -> HashMap<KaniFunction, FnDef> {
    let mut kani = stable_mir::find_crates("kani");
    if kani.is_empty() {
        // In case we are using `kani_core`.
        kani.extend(stable_mir::find_crates("core"));
    }
    kani.into_iter()
        .find_map(|krate| {
            let kani_funcs: HashMap<_, _> = krate
                .fn_defs()
                .into_iter()
                .filter_map(|fn_def| {
                    trace!(?krate, ?fn_def, "find_kani_functions");
                    KaniFunction::try_from(fn_def).ok().map(|kani_function| {
                        debug!(?kani_function, ?fn_def, "Found kani function");
                        (kani_function, fn_def)
                    })
                })
                .collect();
            // All definitions should live in the same crate, so we can return the first one.
            // If there are no definitions, return `None` to indicate that.
            (!kani_funcs.is_empty()).then_some(kani_funcs)
        })
        .unwrap_or_default()
}

/// Ensure we have the valid definitions.
pub fn validate_kani_functions(kani_funcs: &HashMap<KaniFunction, FnDef>) {
    for (kani_function, fn_def) in kani_funcs {
        assert_eq!(KaniFunction::try_from(*fn_def), Ok(*kani_function));
    }
}
