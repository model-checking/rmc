// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Provide passes that perform inter-function analysis on the crate under compilation.
//!
//! This module also includes a `CallGraph` structure to help the analysis.
//! For now, we build the CallGraph as part of the pass, but as we add more analysis,
//! the call-graph could be reused by different analysis.

use crate::analysis::{FnCallVisitor, FnUnsafeOperations, OverallStats};
use stable_mir::mir::{MirVisitor, Safety};
use stable_mir::ty::{FnDef, RigidTy, Ty, TyKind};
use stable_mir::{CrateDef, CrateDefType};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

impl OverallStats {
    /// Iterate over all functions defined in this crate and log any unsafe operation.
    pub fn unsafe_distance(&mut self, filename: PathBuf) {
        let all_items = stable_mir::all_local_items();
        let mut queue =
            all_items.into_iter().filter_map(|item| Node::try_new(item.ty())).collect::<Vec<_>>();
        // Build call graph
        let mut call_graph = CallGraph::default();
        while let Some(node) = queue.pop() {
            if let Entry::Vacant(e) = call_graph.nodes.entry(node.def) {
                e.insert(node);
                let Some(body) = node.def.body() else {
                    continue;
                };
                let mut visitor = FnCallVisitor { body: &body, fns: vec![] };
                visitor.visit_body(&body);
                queue.extend(visitor.fns.iter().map(|def| Node::try_new(def.ty()).unwrap()));
                for callee in &visitor.fns {
                    call_graph.rev_edges.entry(*callee).or_default().push(node.def)
                }
                call_graph.edges.insert(node.def, visitor.fns);
            }
        }

        // Calculate the distance between unsafe functions and functions with unsafe operation.
        let mut queue = call_graph
            .nodes
            .values()
            .filter_map(|node| node.has_unsafe.then_some((node.def, 0)))
            .collect::<VecDeque<_>>();
        let mut visited: HashMap<FnDef, u16> = HashMap::from_iter(queue.iter().cloned());
        while let Some(current) = queue.pop_front() {
            for caller in call_graph.rev_edges.entry(current.0).or_default() {
                if !visited.contains_key(caller) {
                    let distance = current.1 + 1;
                    visited.insert(*caller, distance);
                    queue.push_back((*caller, distance))
                }
            }
        }
        let krate = stable_mir::local_crate();
        let transitive_unsafe = visited
            .into_iter()
            .filter_map(|(def, distance)| (def.krate() == krate).then_some((def.name(), distance)))
            .collect::<Vec<_>>();
        self.counters.push(("transitive_unsafe", transitive_unsafe.len()));
        crate::analysis::dump_csv(filename, &transitive_unsafe);
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Node {
    def: FnDef,
    is_unsafe: bool,
    has_unsafe: bool,
}

impl Node {
    fn try_new(ty: Ty) -> Option<Node> {
        let kind = ty.kind();
        let TyKind::RigidTy(RigidTy::FnDef(def, _)) = kind else {
            return None;
        };
        let has_unsafe = if let Some(body) = def.body() {
            let unsafe_ops = FnUnsafeOperations::new(def.name()).collect(&body);
            unsafe_ops.has_unsafe()
        } else {
            true
        };
        let fn_sig = kind.fn_sig().unwrap();
        let is_unsafe = fn_sig.skip_binder().safety == Safety::Unsafe;
        Some(Node { def, is_unsafe, has_unsafe })
    }
}

impl Hash for Node {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.def.hash(state)
    }
}

#[derive(Default, Debug)]
struct CallGraph {
    nodes: HashMap<FnDef, Node>,
    edges: HashMap<FnDef, Vec<FnDef>>,
    rev_edges: HashMap<FnDef, Vec<FnDef>>,
}
