// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::super::SymbolTable;
use super::gen_c_transformer::{ExprTransformer, NameTransformer, NondetTransformer};
use super::identity_transformer::IdentityTransformer;

/// Performs each pass provided on the given symbol table.
pub fn do_passes(mut symtab: SymbolTable, pass_names: &[String]) -> SymbolTable {
    for pass_name in pass_names {
        symtab = match &pass_name[..] {
            "gen-c" => {
                let symtab = ExprTransformer::transform(&symtab);
                let symtab = NondetTransformer::transform(&symtab);
                let symtab = NameTransformer::transform(&symtab);
                symtab
            }
            "identity" => IdentityTransformer::transform(&symtab),
            _ => panic!("Invalid symbol table transformation: {}", pass_name),
        }
    }

    symtab
}
