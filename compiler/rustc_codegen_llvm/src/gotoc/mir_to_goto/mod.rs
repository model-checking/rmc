// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module takes MIR and emits CBMC goto.

mod assumptions;
mod backend;
mod codegen;
mod context;
mod debug;
mod monomorphize;
mod overrides;
mod utils;

pub use backend::GotocCodegenBackend;
pub use context::GotocCtx;
