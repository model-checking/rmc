// Copyright Kani Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! This module is the central location for handling assertions and assumptions in Kani.
//!
//! There are a few patterns we see with CBMC:
//!
//! 1. A Kani `check` is a CBMC `assert`, which allows execution to proceed if it fails.
//! 2. A Kani `assume` is a CBMC `assume`, thankfully.
//! 3. A Kani `assert` is a CBMC `assert-assume`, first checking the property, then terminating the trace if it fails.
//! 4. A Kani `cover` is a CBMC `assert(!cond)`, that we treat specially in our cbmc output handler.
//!    (We do not use cbmc's notion of a `cover`.)
//!
//! Kani further offers a few special cases:
//!
//! 5. `codegen_unimplemented_{stmt,expr}` : `assert(false)` but recorded specially
//! 6. `codegen_mimic_unimplemented` : for cases where we emit unimplemented, but don't want to log visibly
//! 7. `codegen_sanity` : `assert` but not normally displayed as failure would be a Kani bug
//!

use crate::codegen_cprover_gotoc::GotocCtx;
use cbmc::goto_program::{Expr, Location, Stmt, Type};
use cbmc::InternedString;
use stable_mir::ty::Span as SpanStable;
use strum_macros::{AsRefStr, EnumString};
use tracing::debug;

/// Classifies the type of CBMC `assert`, as different assertions can have different semantics (e.g. cover)
///
/// Each property class should justify its existence with a note about the special handling it recieves.
#[derive(Debug, Clone, EnumString, AsRefStr, PartialEq)]
#[strum(serialize_all = "snake_case")]
pub enum PropertyClass {
    /// Overflow panics that can be generated by Intrisics.
    /// NOTE: Not all uses of this are found by rust-analyzer because of the use of macros. Try grep instead.
    ///
    /// SPECIAL BEHAVIOR: None TODO: Why should this exist?
    ArithmeticOverflow,
    /// The Rust `assume` instrinsic is `assert`'d by Kani, and gets this property class.
    ///
    /// SPECIAL BEHAVIOR: None? Possibly confusing to customers that a Rust assume is a Kani assert.
    Assume,
    /// See [GotocCtx::codegen_cover] below. Generally just an `assert(false)` that's not an error.
    ///
    /// SPECIAL BEHAVIOR: "Errors" for this type of assertion just mean "reachable" not failure.
    Cover,
    /// The class of checks used for code coverage instrumentation. Only needed
    /// when working on coverage-related features.
    ///
    /// Do not mistake with `Cover`, they are different:
    ///  - `CodeCoverage` checks have a fixed condition (`false`) and description.
    ///  - `CodeCoverage` checks are filtered out from verification results and
    ///    postprocessed to build coverage reports.
    ///  - `Cover` checks can be added by users (using the `kani::cover` macro),
    ///    while `CodeCoverage` checks are not exposed to users (i.e., they are
    ///    automatically added if running with the coverage option).
    ///
    /// SPECIAL BEHAVIOR: "Errors" for this type of assertion just mean "reachable" not failure.
    CodeCoverage,
    /// Ordinary (Rust) assertions and panics.
    ///
    /// SPECIAL BEHAVIOR: These assertion failures should be observable during normal execution of Rust code.
    /// That is, they do not depend on special instrumentation that Kani performs that wouldn't
    /// otherwise be observable.
    Assertion,
    /// Another instrinsic check.
    ///
    /// SPECIAL BEHAVIOR: None TODO: Why should this exist?
    ExactDiv,
    /// Another instrinsic check.
    ///
    /// SPECIAL BEHAVIOR: None TODO: Why should this exist?
    FiniteCheck,
    /// Checks added by Kani compiler to determine whether a property (e.g.
    /// `PropertyClass::Assertion` or `PropertyClass:Cover`) is reachable
    ReachabilityCheck,
    /// Checks added by Kani compiler to detect safety conditions violation.
    /// E.g., things that trigger UB or unstable behavior.
    ///
    /// SPECIAL BEHAVIOR: Assertions that may not exist when running code normally (i.e. not under Kani)
    SafetyCheck,
    /// Checks to ensure that Kani's code generation is correct.
    ///
    /// SPECIAL BEHAVIOR: Should not be normally rendered as a checked assertion, as it's expected to succeed.
    SanityCheck,
    /// See `codegen_unimplemented`. Used to indicate an unsupported construct was reachable.
    ///
    /// SPECIAL BEHAVIOR: Reachability of these assertions is notable, in order to measure Kani support.
    /// Also makes other properties UNDETERMINED.
    UnsupportedConstruct,
    /// When Rust determines code is unreachable, this is the `assert(false)` we emit.
    ///
    /// SPECIAL BEHAVIOR: Kinda should be a SanityCheck, except that we emit it also for
    /// `std::intrinsics::unreachable()` and can't tell the difference between that case
    /// and other cases where the Rust compiler thinks things should be unreachable.
    Unreachable,
}

#[allow(dead_code)]
impl PropertyClass {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl<'tcx> GotocCtx<'tcx> {
    /// Generates a CBMC assertion. Note: Does _NOT_ assume.
    pub fn codegen_assert(
        &self,
        cond: Expr,
        property_class: PropertyClass,
        message: &str,
        loc: Location,
    ) -> Stmt {
        let property_name = property_class.as_str();
        Stmt::assert(cond, property_name, message, loc)
    }

    /// Generates a CBMC assumption.
    pub fn codegen_assume(&self, cond: Expr, loc: Location) -> Stmt {
        Stmt::assume(cond, loc)
    }

    /// Generates a CBMC assertion, followed by an assumption of the same condition.
    pub fn codegen_assert_assume(
        &self,
        cond: Expr,
        property_class: PropertyClass,
        message: &str,
        loc: Location,
    ) -> Stmt {
        let property_name = property_class.as_str();
        Stmt::block(
            vec![Stmt::assert(cond.clone(), property_name, message, loc), Stmt::assume(cond, loc)],
            loc,
        )
    }

    /// Generate code to cover the given condition at the current location
    pub fn codegen_cover(&self, cond: Expr, msg: &str, span: SpanStable) -> Stmt {
        let loc = self.codegen_caller_span_stable(span);
        // Should use Stmt::cover, but currently this doesn't work with CBMC
        // unless it is run with '--cover cover' (see
        // https://github.com/diffblue/cbmc/issues/6613). So for now use
        // assert(!cond).
        self.codegen_assert(cond.not(), PropertyClass::Cover, msg, loc)
    }

    /// Generate a cover statement for code coverage reports.
    pub fn codegen_coverage(&self, span: SpanStable) -> Stmt {
        let loc = self.codegen_caller_span_stable(span);
        // Should use Stmt::cover, but currently this doesn't work with CBMC
        // unless it is run with '--cover cover' (see
        // https://github.com/diffblue/cbmc/issues/6613). So for now use
        // `assert(false)`.
        self.codegen_assert(
            Expr::bool_false(),
            PropertyClass::CodeCoverage,
            "code coverage for location",
            loc,
        )
    }

    // The above represent the basic operations we can perform w.r.t. assert/assume/cover
    // Below are various helper functions for constructing the above more easily.

    /// Given the message for a property, generate a reachability check that is
    /// meant to check whether the property is reachable. The function returns a
    /// modified version of the provided message that should be used for the
    /// property to allow the CBMC output parser to pair the property with its
    /// reachability check.
    /// If reachability checks are disabled, the function returns the message
    /// unmodified and an empty (skip) statement.
    pub fn codegen_reachability_check(&mut self, msg: String, span: SpanStable) -> (String, Stmt) {
        let loc = self.codegen_caller_span_stable(span);
        if self.queries.args().check_assertion_reachability {
            // Generate a unique ID for the assert
            let assert_id = self.next_check_id();
            // Also add the unique ID as a prefix to the assert message so that it can be
            // easily paired with the reachability check
            let msg = GotocCtx::add_prefix_to_msg(&msg, &assert_id);
            // Generate a message for the reachability check that includes the unique ID
            let reach_msg = assert_id;
            // inject a reachability check, which is a (non-blocking)
            // assert(false) whose failure indicates that this line is reachable.
            // The property class (`PropertyClass:ReachabilityCheck`) is used by
            // the CBMC output parser to distinguish those checks from others.
            let check = self.codegen_assert(
                Expr::bool_false(),
                PropertyClass::ReachabilityCheck,
                &reach_msg,
                loc,
            );
            (msg, check)
        } else {
            (msg, Stmt::skip(loc))
        }
    }

    /// A shorthand for generating a CBMC assert-assume(false)
    pub fn codegen_assert_assume_false(
        &self,
        property_class: PropertyClass,
        message: &str,
        loc: Location,
    ) -> Stmt {
        self.codegen_assert_assume(Expr::bool_false(), property_class, message, loc)
    }

    /// A shorthand for assert-assume(false) that takes a MIR `Span` instead of a CBMC `Location`.
    pub fn codegen_fatal_error(
        &self,
        property_class: PropertyClass,
        msg: &str,
        span: SpanStable,
    ) -> Stmt {
        let loc = self.codegen_caller_span_stable(span);
        self.codegen_assert_assume_false(property_class, msg, loc)
    }

    /// Kani hooks function calls to `panic` and calls this intead.
    pub fn codegen_panic(&self, span: SpanStable, fargs: Vec<Expr>) -> Stmt {
        // CBMC requires that the argument to the assertion must be a string constant.
        // If there is one in the MIR, use it; otherwise, explain that we can't.
        assert!(!fargs.is_empty(), "Panic requires a string message");
        let msg = self.extract_const_message(&fargs[0]).unwrap_or(String::from(
            "This is a placeholder message; Kani doesn't support message formatted at runtime",
        ));
        self.codegen_fatal_error(PropertyClass::Assertion, &msg, span)
    }

    /// Kani does not currently support all MIR constructs.
    ///
    /// This action will
    ///
    /// 1. Fail verification in a machine-detectable manner if reachable
    /// 2. Warn about unsupported features at compile-time
    ///
    /// Because this appears in an expression context, it will technically return a
    /// nondet value of the requested type. However, control flow will not actually
    /// proceed from this assertion failure.
    pub fn codegen_unimplemented_expr(
        &mut self,
        operation_name: &str,
        t: Type,
        loc: Location,
        url: &str,
    ) -> Expr {
        let body = vec![
            self.codegen_unimplemented_stmt(operation_name, loc, url),
            t.nondet().as_stmt(loc),
        ];

        Expr::statement_expression(body, t).with_location(loc)
    }

    /// Kani does not currently support all MIR constructs.
    ///
    /// This action will
    ///
    /// 1. Fail verification in a machine-detectable manner if reachable
    /// 2. Warn about unsupported features at compile-time
    ///
    /// Control flow will not proceed forward from this assertion failure.
    pub fn codegen_unimplemented_stmt(
        &mut self,
        operation_name: &str,
        loc: Location,
        url: &str,
    ) -> Stmt {
        debug!("codegen_unimplemented: {} at {}", operation_name, loc.short_string());

        // Save this occurrence so we can emit a warning in the compilation report.
        let key: InternedString = operation_name.into();
        let entry = self.unsupported_constructs.entry(key).or_default();
        entry.push(loc);

        self.codegen_assert_assume(
            Expr::bool_false(),
            PropertyClass::UnsupportedConstruct,
            &GotocCtx::unsupported_msg(operation_name, Some(url)),
            loc,
        )
    }

    /// There are a handful of location where we want to codegen unimplemented... but also
    /// not really report these statically to the user on compilation. This does exactly
    /// the same thing as `codegen_unimplemented_stmt` but doesn't add it to the list
    /// of `unsupported_constructs`
    ///
    /// TODO: Ideally we'd eliminate this. Currently used in two places:
    ///
    /// - `TerminatorKind::Resume` and `TerminatorKind::Abort`. Related to unwind support.
    pub fn codegen_mimic_unimplemented(
        &mut self,
        operation_name: &str,
        loc: Location,
        url: &str,
    ) -> Stmt {
        debug!("codegen_mimic_unimplemented: {} at {}", operation_name, loc.short_string());

        // TODO: We DO want to record this in kani-metadata, but we DON'T want to bother users about it with a message

        self.codegen_assert_assume(
            Expr::bool_false(),
            PropertyClass::UnsupportedConstruct,
            &GotocCtx::unsupported_msg(operation_name, Some(url)),
            loc,
        )
    }

    /// Assertion that should always be true unless there is a bug in Kani.
    ///
    /// Not normally rendered as a property being checked to the user, and
    /// includes a bug-filing link for Kani if it fails.
    pub fn codegen_sanity(&self, cond: Expr, message: &str, loc: Location) -> Stmt {
        pub const BUG_REPORT_URL: &str =
            "https://github.com/model-checking/kani/issues/new?template=bug_report.md";

        let assert_msg = format!(
            "Kani-internal sanity check: {message}. Please report failures:\n{BUG_REPORT_URL}"
        );

        self.codegen_assert_assume(cond, PropertyClass::SanityCheck, &assert_msg, loc)
    }
}
