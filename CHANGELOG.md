# Changelog

This file contains notable changes (e.g. breaking changes, major changes, etc.) in Kani releases.

This file was introduced starting Kani 0.23.0, so it only contains changes from version 0.23.0 onwards.

## [0.60.0]

### Breaking Changes
* Autoharness Subcommand by @carolynzech in https://github.com/model-checking/kani/pull/3874

### What's Changed
* Fast fail feature - Stops verification process as soon as one failure is observed - Use case : CI speed up by @rajath-mk in https://github.com/model-checking/kani/pull/3879
* Adjust PropertyClass of assertions to identify UB by @tautschnig in https://github.com/model-checking/kani/pull/3860
* Remove isize overflow check for zst offsets by @carolynzech in https://github.com/model-checking/kani/pull/3897
* Convert raw URL to link by @flba-eb in https://github.com/model-checking/kani/pull/3907
* Update toolchain to 2025-03-02 by @remi-delmas-3000 @carolynzech @thanhnguyen-aws @zhassan-aws and @tautschnig

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.59.0...kani-0.60.0

## [0.59.0]

### Breaking Changes
* Deprecate `--enable-unstable` and `--restrict-vtable` by @celinval in https://github.com/model-checking/kani/pull/3859
* Do not report arithmetic overflow for floating point operations that produce +/-Inf by @rajath-mk in https://github.com/model-checking/kani/pull/3873

### What's Changed
* Fix validity checks for `char` by @celinval in https://github.com/model-checking/kani/pull/3853
* Support verifying contracts/stubs for generic types with multiple inherent implementations by @carolynzech in https://github.com/model-checking/kani/pull/3829
* Allow multiple stub_verified annotations, but check for duplicate targets by @remi-delmas-3000 in https://github.com/model-checking/kani/pull/3808
* Fix crash if a function pointer is created but never used by @celinval in https://github.com/model-checking/kani/pull/3862
* Fix transmute codegen when sizes are different by @celinval in https://github.com/model-checking/kani/pull/3861
* Stub linker to avoid missing symbols errors by @celinval in https://github.com/model-checking/kani/pull/3858
* Toolchain upgrade to nightly-2025-01-28 by @feliperodri @tautschnig

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.58.0...kani-0.59.0

## [0.58.0]

### Major/Breaking Changes
* Improve `--jobs` UI by @carolynzech in https://github.com/model-checking/kani/pull/3790
* Generate contracts of dependencies as assertions by @carolynzech in https://github.com/model-checking/kani/pull/3802
* Add UB checks for ptr_offset_from* intrinsics by @celinval in https://github.com/model-checking/kani/pull/3757

### What's Changed
* Include manifest-path when checking if packages are in the workspace by @qinheping in https://github.com/model-checking/kani/pull/3819
* Update kissat to v4.0.1 by @remi-delmas-3000 in https://github.com/model-checking/kani/pull/3791
* Rust toolchain upgraded to 2025-01-07 by @remi-delmas-3000 @zhassan-aws

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.57.0...kani-0.58.0

## [0.57.0]

### Major Changes
* `kani-cov`: A coverage tool for Kani by @adpaco-aws in https://github.com/model-checking/kani/pull/3121
* Add a timeout option by @zhassan-aws in https://github.com/model-checking/kani/pull/3649
* Loop Contracts Annotation for While-Loop by @qinheping in https://github.com/model-checking/kani/pull/3151
* Harness output individual files by @Alexander-Aghili in https://github.com/model-checking/kani/pull/3360
* Enable support for Ubuntu 24.04 by @tautschnig in https://github.com/model-checking/kani/pull/3758

### Breaking Changes
* Make `kani::check` private by @celinval in https://github.com/model-checking/kani/pull/3614
* Remove symtab json support by @celinval in https://github.com/model-checking/kani/pull/3695
* Remove CBMC viewer and visualize option by @zhassan-aws in https://github.com/model-checking/kani/pull/3699
* Dropping support for Ubuntu 18.04 / AL2. by @thanhnguyen-aws in https://github.com/model-checking/kani/pull/3744

### What's Changed
* Remove the overflow checks for wrapping_offset by @zhassan-aws in https://github.com/model-checking/kani/pull/3589
* Support fully-qualified --package arguments by @celinval in https://github.com/model-checking/kani/pull/3593
* Implement proper function pointer handling for validity checks by @celinval in https://github.com/model-checking/kani/pull/3606
* Add fn that checks pointers point to same allocation by @celinval in https://github.com/model-checking/kani/pull/3583
* [Lean back-end] Preserve variable names by @zhassan-aws in https://github.com/model-checking/kani/pull/3560
* Emit an error when proof_for_contract function is not found by @zhassan-aws in https://github.com/model-checking/kani/pull/3609
* [Lean back-end] Rename user-facing options from Aeneas to Lean by @zhassan-aws in https://github.com/model-checking/kani/pull/3630
* Fix ICE due to mishandling of Aggregate rvalue for raw pointers to trait objects by @carolynzech in https://github.com/model-checking/kani/pull/3636
* Fix loop contracts transformation when loops in branching by @qinheping in https://github.com/model-checking/kani/pull/3640
* Move any_slice_from_array to kani_core by @qinheping in https://github.com/model-checking/kani/pull/3646
* Implement `Arbitrary` for `Range*` by @c410-f3r in https://github.com/model-checking/kani/pull/3666
* Add support for float_to_int_unchecked by @zhassan-aws in https://github.com/model-checking/kani/pull/3660
* Change `same_allocation` to accept wide pointers by @celinval in https://github.com/model-checking/kani/pull/3684
* Derive `Arbitrary` for enums with a single variant by @AlgebraicWolf in https://github.com/model-checking/kani/pull/3692
* Apply loop contracts only if there exists some usage by @qinheping in https://github.com/model-checking/kani/pull/3694
* Add support for f16 and f128 in float_to_int_unchecked intrinsic by @zhassan-aws in https://github.com/model-checking/kani/pull/3701
* Fix codegen for rvalue aggregate raw pointer to an adt with slice tail by @carolynzech in https://github.com/model-checking/kani/pull/3644
* Improve Kani handling of function markers by @celinval in https://github.com/model-checking/kani/pull/3718
* Enable contracts for const generic functions by @qinheping in https://github.com/model-checking/kani/pull/3726
* List Subcommand Improvements by @carolynzech in https://github.com/model-checking/kani/pull/3729
* [Lean back-end] add support for enum, struct, tuple in llbc backend by @thanhnguyen-aws in https://github.com/model-checking/kani/pull/3721
* Fix issues with how we compute DST size by @celinval in https://github.com/model-checking/kani/pull/3687
* Fix size and alignment computation for intrinsics by @celinval in https://github.com/model-checking/kani/pull/3734
* Add a Kani function that checks if the range of a float is valid for conversion to int by @zhassan-aws in https://github.com/model-checking/kani/pull/3742
* Add out of bounds check for `offset` intrinsics by @celinval in https://github.com/model-checking/kani/pull/3755
* Automatic upgrade of CBMC from 6.3.1 to 6.4.1
* Rust toolchain upgraded to nightly-2024-12-15 by @zhassan-aws @carolynzech @qinheping @celinval @tautschnig

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.56.0...kani-0.57.0

## [0.56.0]

### Major/Breaking Changes

* Remove obsolete linker options (`--mir-linker` and `--legacy-linker`) by @zhassan-aws in https://github.com/model-checking/kani/pull/3559
* List Subcommand by @carolynzech in https://github.com/model-checking/kani/pull/3523
* Deprecate `kani::check` by @celinval in https://github.com/model-checking/kani/pull/3557

### What's Changed

* Enable stubbing and function contracts for primitive types by @celinval in https://github.com/model-checking/kani/pull/3496
* Instrument validity checks for pointer to reference casts for slices and str's by @zhassan-aws in https://github.com/model-checking/kani/pull/3513
* Fail compilation if `proof_for_contract` is added to generic function by @carolynzech in https://github.com/model-checking/kani/pull/3522
* Fix storing coverage data in cargo projects by @adpaco-aws in https://github.com/model-checking/kani/pull/3527
* Add experimental API to generate arbitrary pointers by @celinval in https://github.com/model-checking/kani/pull/3538
* Running `verify-std` no longer changes Cargo files by @celinval in https://github.com/model-checking/kani/pull/3577
* Add an LLBC backend by @zhassan-aws in https://github.com/model-checking/kani/pull/3514
* Fix the computation of the number of bytes of a pointer offset by @zhassan-aws in https://github.com/model-checking/kani/pull/3584
* Rust toolchain upgraded to nightly-2024-10-03 by @qinheping @tautschnig @celinval
* CBMC upgraded to 6.3.1 by @tautschnig in https://github.com/model-checking/kani/pull/3537

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.55.0...kani-0.56.0

## [0.55.0]

### Major/Breaking Changes
* Coverage reporting in Kani is now source-based instead of line-based.
Consequently, the unstable `-Zline-coverage` flag has been replaced with a `-Zsource-coverage` one.
Check the [Source-Coverage RFC](https://model-checking.github.io/kani/rfc/rfcs/0011-source-coverage.html) for more details.
* Several improvements were made to the memory initialization checks. The current state is summarized in https://github.com/model-checking/kani/issues/3300. We welcome your feedback!

### What's Changed
* Update CBMC build instructions for Amazon Linux 2 by @tautschnig in https://github.com/model-checking/kani/pull/3431
* Implement memory initialization state copy functionality by @artemagvanian in https://github.com/model-checking/kani/pull/3350
* Make points-to analysis handle all intrinsics explicitly by @artemagvanian in https://github.com/model-checking/kani/pull/3452
* Avoid corner-cases by grouping instrumentation into basic blocks and using backward iteration by @artemagvanian in https://github.com/model-checking/kani/pull/3438
* Fix ICE due to mishandling of Aggregate rvalue for raw pointers to `str` by @celinval in https://github.com/model-checking/kani/pull/3448
* Basic support for memory initialization checks for unions by @artemagvanian in https://github.com/model-checking/kani/pull/3444
* Adopt Rust's source-based code coverage instrumentation by @adpaco-aws in https://github.com/model-checking/kani/pull/3119
* Extra tests and bug fixes to the delayed UB instrumentation by @artemagvanian in https://github.com/model-checking/kani/pull/3419
* Partially integrate uninit memory checks into `verify_std` by @artemagvanian in https://github.com/model-checking/kani/pull/3470
* Rust toolchain upgraded to `nightly-2024-09-03` by @jaisnan @carolynzech 

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.54.0...kani-0.55.0

## [0.54.0]

### Major Changes
* We added support for slices in the `#[kani::modifies(...)]` clauses when using function contracts.
* We introduce an `#[safety_constraint(...)]` attribute helper for the `Arbitrary` and `Invariant` macros.
* We enabled support for concrete playback for harness that contains stubs or function contracts.
* We added support for log2*, log10*, powif*, fma*, and sqrt* intrisincs.

### Breaking Changes
* The `-Z ptr-to-ref-cast-checks` option has been removed, and pointer validity checks when casting raw pointers to references are now run by default.

## What's Changed
* Make Kani reject mutable pointer casts if padding is incompatible and memory initialization is checked by @artemagvanian in https://github.com/model-checking/kani/pull/3332
* Fix visibility of some Kani intrinsics by @artemagvanian in https://github.com/model-checking/kani/pull/3323
* Function Contracts: Modify Slices by @pi314mm in https://github.com/model-checking/kani/pull/3295
* Support for disabling automatically generated pointer checks to avoid reinstrumentation by @artemagvanian in https://github.com/model-checking/kani/pull/3344
* Add support for global transformations by @artemagvanian in https://github.com/model-checking/kani/pull/3348
* Enable an `#[safety_constraint(...)]` attribute helper for the `Arbitrary` and `Invariant` macros by @adpaco-aws in https://github.com/model-checking/kani/pull/3283
* Fix contract handling of promoted constants and constant static by @celinval in https://github.com/model-checking/kani/pull/3305
* Bump CBMC Viewer to 3.9 by @tautschnig in https://github.com/model-checking/kani/pull/3373
* Update to CBMC version 6.1.1 by @tautschnig in https://github.com/model-checking/kani/pull/2995
* Define a struct-level `#[safety_constraint(...)]` attribute by @adpaco-aws in https://github.com/model-checking/kani/pull/3270
* Enable concrete playback for contract and stubs by @celinval in https://github.com/model-checking/kani/pull/3389
* Add code scanner tool by @celinval in https://github.com/model-checking/kani/pull/3120
* Enable contracts in associated functions by @celinval in https://github.com/model-checking/kani/pull/3363
* Enable log2*, log10* intrinsics by @tautschnig in https://github.com/model-checking/kani/pull/3001
* Enable powif* intrinsics by @tautschnig in https://github.com/model-checking/kani/pull/2999
* Enable fma* intrinsics by @tautschnig in https://github.com/model-checking/kani/pull/3002
* Enable sqrt* intrinsics by @tautschnig in https://github.com/model-checking/kani/pull/3000
* Remove assigns clause for ZST pointers by @carolynzech in https://github.com/model-checking/kani/pull/3417
* Instrumentation for delayed UB stemming from uninitialized memory by @artemagvanian in https://github.com/model-checking/kani/pull/3374
* Unify kani library and kani core logic by @jaisnan in https://github.com/model-checking/kani/pull/3333
* Stabilize pointer-to-reference cast validity checks by @artemagvanian in https://github.com/model-checking/kani/pull/3426
* Rust toolchain upgraded to `nightly-2024-08-07` by @jaisnan @qinheping @tautschnig @feliperodri

## New Contributors
* @carolynzech made their first contribution in https://github.com/model-checking/kani/pull/3387

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.53.0...kani-0.54.0

## [0.53.0]

### Major Changes
* The `--visualize` option is being deprecated and will be removed in a future release. Consider using the `--concrete-playback` option instead.
* The `-Z ptr-to-ref-cast-checks` option is being introduced to check pointer validity when casting raw pointers to references. The feature is currently behind an unstable flag but is expected to be stabilized in a future release once remaining performance issues have been resolved.
* The `-Z uninit-checks` option is being introduced to check memory initialization. The feature is currently behind an unstable flag and also requires the `-Z ghost-state` option.

### Breaking Changes
* Remove support for the unstable argument `--function` by @celinval in https://github.com/model-checking/kani/pull/3278
* Remove deprecated `--enable-stubbing` by @celinval in https://github.com/model-checking/kani/pull/3309

### What's Changed

* Change ensures into closures by @pi314mm in https://github.com/model-checking/kani/pull/3207
* (Re)introduce `Invariant` trait by @adpaco-aws in https://github.com/model-checking/kani/pull/3190
* Remove empty box creation from contracts impl by @celinval in https://github.com/model-checking/kani/pull/3233
* Add a new verify-std subcommand to Kani by @celinval in https://github.com/model-checking/kani/pull/3231
* Inject pointer validity check when casting raw pointers to references by @artemagvanian in https://github.com/model-checking/kani/pull/3221
* Do not turn trivially diverging loops into assume(false) by @tautschnig in https://github.com/model-checking/kani/pull/3223
* Fix "unused mut" warnings created by generated code. by @jsalzbergedu in https://github.com/model-checking/kani/pull/3247
* Refactor stubbing so Kani compiler only invoke rustc once per crate by @celinval in https://github.com/model-checking/kani/pull/3245
* Use cfg=kani_host for host crates by @tautschnig in https://github.com/model-checking/kani/pull/3244
* Add intrinsics and Arbitrary support for no_core by @jaisnan in https://github.com/model-checking/kani/pull/3230
* Contracts: Avoid attribute duplication and `const` function generation for constant function by @celinval in https://github.com/model-checking/kani/pull/3255
* Fix contract of constant fn with effect feature by @celinval in https://github.com/model-checking/kani/pull/3259
* Fix typed_swap for ZSTs by @tautschnig in https://github.com/model-checking/kani/pull/3256
* Add a `#[derive(Invariant)]` macro by @adpaco-aws in https://github.com/model-checking/kani/pull/3250
* Contracts: History Expressions via "old" monad by @pi314mm in https://github.com/model-checking/kani/pull/3232
* Function Contracts: remove instances of _renamed by @pi314mm in https://github.com/model-checking/kani/pull/3274
* Deprecate `--visualize` in favor of concrete playback by @celinval in https://github.com/model-checking/kani/pull/3281
* Fix operand in fat pointer comparison by @pi314mm in https://github.com/model-checking/kani/pull/3297
* Function Contracts: Closure Type Inference by @pi314mm in https://github.com/model-checking/kani/pull/3307
* Add support for f16 and f128 for toolchain upgrade to 6/28 by @jaisnan in https://github.com/model-checking/kani/pull/3306
* Towards Proving Memory Initialization by @artemagvanian in https://github.com/model-checking/kani/pull/3264
* Rust toolchain upgraded to `nightly-2024-07-01` by @tautschnig @celinval @jaisnan @adpaco-aws

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.52.0...kani-0.53.0

## [0.52.0]

## What's Changed
* New section about linter configuraton checking in the doc by @remi-delmas-3000 in https://github.com/model-checking/kani/pull/3198
* Fix `{,e}println!()` by @GrigorenkoPV in https://github.com/model-checking/kani/pull/3209
* Contracts for a few core functions by @celinval in https://github.com/model-checking/kani/pull/3107
* Add simple API for shadow memory by @zhassan-aws in https://github.com/model-checking/kani/pull/3200
* Upgrade Rust toolchain to 2024-05-28 by @zhassan-aws @remi-delmas-3000 @qinheping

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.51.0...kani-0.52.0

## [0.51.0]

### What's Changed

* Do not assume that ZST-typed symbols refer to unique objects by @tautschnig in https://github.com/model-checking/kani/pull/3134
* Remove `kani::Arbitrary` from the `modifies` contract instrumentation by @feliperodri in https://github.com/model-checking/kani/pull/3169
* Emit source locations whenever possible to ease debugging and coverage reporting by @tautschnig in https://github.com/model-checking/kani/pull/3173
* Rust toolchain upgraded to `nightly-2024-04-21` by @celinval


**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.50.0...kani-0.51.0


## [0.50.0]

### Major Changes
* Fix compilation issue with proc_macro2  (v1.0.80+) and Kani v0.49.0
(https://github.com/model-checking/kani/issues/3138).

### What's Changed
* Implement valid value check for `write_bytes` by @celinval in
https://github.com/model-checking/kani/pull/3108
* Rust toolchain upgraded to 2024-04-15 by @tautschnig @celinval

**Full Changelog**:
https://github.com/model-checking/kani/compare/kani-0.49.0...kani-0.50.0

## [0.49.0]

### What's Changed
* Disable removal of storage markers by @zhassan-aws in https://github.com/model-checking/kani/pull/3083
* Ensure storage markers are kept in std code by @zhassan-aws in https://github.com/model-checking/kani/pull/3080
* Implement validity checks by @celinval in https://github.com/model-checking/kani/pull/3085
* Allow modifies clause for verification only by @feliperodri in https://github.com/model-checking/kani/pull/3098
* Add optional scatterplot to benchcomp output by @tautschnig in https://github.com/model-checking/kani/pull/3077
* Expand ${var} in benchcomp variant `env` by @karkhaz in https://github.com/model-checking/kani/pull/3090
* Add `benchcomp filter` command by @karkhaz in https://github.com/model-checking/kani/pull/3105
* Upgrade Rust toolchain to 2024-03-29 by @zhassan-aws @celinval @adpaco-aws @feliperodri

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.48.0...kani-0.49.0

## [0.48.0]

### Major Changes
* We fixed a soundness bug that in some cases may cause Kani to not detect a use-after-free issue in https://github.com/model-checking/kani/pull/3063

### What's Changed
* Fix `codegen_atomic_binop` for `atomic_ptr` by @qinheping in https://github.com/model-checking/kani/pull/3047
* Retrieve info for recursion tracker reliably by @feliperodri in https://github.com/model-checking/kani/pull/3045
* Add `--use-local-toolchain` to Kani setup by @jaisnan in https://github.com/model-checking/kani/pull/3056
* Replace internal reverse_postorder by a stable one by @celinval in https://github.com/model-checking/kani/pull/3064
* Add option to override `--crate-name` from `kani` by @adpaco-aws in https://github.com/model-checking/kani/pull/3054
* Rust toolchain upgraded to 2024-03-11 by @adpaco-ws @celinval @zyadh

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.47.0...kani-0.48.0

## [0.47.0]

### What's Changed
* Upgrade toolchain to 2024-02-14 by @zhassan-aws in https://github.com/model-checking/kani/pull/3036

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.46.0...kani-0.47.0

## [0.46.0]

## What's Changed
* `modifies` Clauses for Function Contracts by @JustusAdam in https://github.com/model-checking/kani/pull/2800
* Fix ICEs due to mismatched arguments by @celinval in https://github.com/model-checking/kani/pull/2994. Resolves the following issues:
  * https://github.com/model-checking/kani/issues/2260
  * https://github.com/model-checking/kani/issues/2312
* Enable powf*, exp*, log* intrinsics by @tautschnig in https://github.com/model-checking/kani/pull/2996
* Upgrade Rust toolchain to nightly-2024-01-24 by @celinval @feliperodri @qinheping

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.45.0...kani-0.46.0

## [0.45.0]

## What's Changed
* Upgrade toolchain to nightly-2024-01-17 by @celinval in https://github.com/model-checking/kani/pull/2976

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.44.0...kani-0.45.0

## [0.44.0]

### What's Changed

* Rust toolchain upgraded to `nightly-2024-01-08` by @adpaco-aws @celinval @zhassan-aws

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.43.0...kani-0.44.0

## [0.43.0]

###  What's Changed
* Rust toolchain upgraded to `nightly-2023-12-14` by @tautschnig and @adpaco-aws

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.42.0...kani-0.43.0

## [0.42.0]

### What's Changed

* Build CBMC from source and install as package on non-x86_64 by @bennofs in https://github.com/model-checking/kani/pull/2877 and https://github.com/model-checking/kani/pull/2878
* Emit suggestions and an explanation when CBMC runs out of memory by @JustusAdam in https://github.com/model-checking/kani/pull/2885
* Rust toolchain upgraded to `nightly-2023-11-28` by @celinval

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.41.0...kani-0.42.0

## [0.41.0]

### Breaking Changes

* Set minimum python to 3.7 in docker container and release action by @remi-delmas-3000 in https://github.com/model-checking/kani/pull/2879
* Delete `any_slice` which has been deprecated since Kani 0.38.0. by @zhassan-aws in https://github.com/model-checking/kani/pull/2860

### What's Changed

* Make `cover` const by @jswrenn in https://github.com/model-checking/kani/pull/2867
* Change `expect()` from taking formatted strings to use `unwrap_or_else()` by @matthiaskrgr in https://github.com/model-checking/kani/pull/2865
* Fix setup for `aarch64-unknown-linux-gnu` platform by @adpaco-aws in https://github.com/model-checking/kani/pull/2864
* Do not override `std` library during playback by @celinval in https://github.com/model-checking/kani/pull/2852
* Rust toolchain upgraded to `nightly-2023-11-11` by @zhassan-aws

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.40.0...kani-0.41.0

## [0.40.0]

### What's Changed

* Ease setup in Amazon Linux 2 by @adpaco-aws in https://github.com/model-checking/kani/pull/2833
* Propagate backend options into goto-synthesizer by @qinheping in https://github.com/model-checking/kani/pull/2643
* Update CBMC version to 5.95.1 by @adpaco-aws in https://github.com/model-checking/kani/pull/2844
* Rust toolchain upgraded to `nightly-2023-10-31` by @jaisnan @adpaco-aws

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.39.0...kani-0.40.0

## [0.39.0]

### What's Changed

* Limit --exclude to workspace packages by @tautschnig in https://github.com/model-checking/kani/pull/2808
* Fix panic warning and add arbitrary Duration by @celinval in https://github.com/model-checking/kani/pull/2820
* Update CBMC version to 5.94 by @celinval in https://github.com/model-checking/kani/pull/2821
* Rust toolchain upgraded to `nightly-2023-10-17` by @celinval @tautschnig

**Full Changelog**:
https://github.com/model-checking/kani/compare/kani-0.38.0...kani-0.39.0

## [0.38.0]

### Major Changes

* Deprecate `any_slice` by @zhassan-aws in https://github.com/model-checking/kani/pull/2789

### What's Changed

* Provide better error message for invalid stubs by @JustusAdam in https://github.com/model-checking/kani/pull/2787
* Simple Stubbing with Contracts by @JustusAdam in https://github.com/model-checking/kani/pull/2746
* Avoid mismatch when generating structs that represent scalar data but also include ZSTs by @adpaco-aws in https://github.com/model-checking/kani/pull/2794
* Prevent kani crash during setup for first time by @jaisnan in https://github.com/model-checking/kani/pull/2799
* Create concrete playback temp files in source directory by @tautschnig in https://github.com/model-checking/kani/pull/2804
* Bump CBMC version by @zhassan-aws in https://github.com/model-checking/kani/pull/2796
* Update Rust toolchain to 2023-09-23 by @tautschnig in https://github.com/model-checking/kani/pull/2806

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.37.0...kani-0.38.0

## [0.37.0]

### Major Changes

* Delete obsolete stubs for `Vec` and related options by @zhassan-aws in https://github.com/model-checking/kani/pull/2770
* Add support for the ARM64 Linux platform by @adpaco-aws in https://github.com/model-checking/kani/pull/2757

### What's Changed

* Function Contracts: Support for defining and checking `requires` and `ensures` clauses by @JustusAdam in https://github.com/model-checking/kani/pull/2655
* Force `any_vec` capacity to match length by @celinval in https://github.com/model-checking/kani/pull/2765
* Fix expected value for `pref_align_of` under aarch64/macos by @remi-delmas-3000 in https://github.com/model-checking/kani/pull/2782
* Bump CBMC version to 5.92.0 by @zhassan-aws in https://github.com/model-checking/kani/pull/2771
* Upgrade to Kissat 3.1.1 by @zhassan-aws in https://github.com/model-checking/kani/pull/2756
* Rust toolchain upgraded to `nightly-2023-09-19` by @remi-delmas-3000 @tautschnig

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.36.0...kani-0.37.0

## [0.36.0]

### What's Changed

* Enable `-Z stubbing` and error out instead of ignoring stub by @celinval in https://github.com/model-checking/kani/pull/2678
* Enable concrete playback for failure of UB checks by @zhassan-aws in https://github.com/model-checking/kani/pull/2727
* Bump CBMC version to 5.91.0 by @adpaco-aws in https://github.com/model-checking/kani/pull/2733
* Rust toolchain upgraded to `nightly-2023-09-06` by @celinval @jaisnan @adpaco-aws

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.35.0...kani-0.36.0

## [0.35.0]

### What's Changed

* Add support to `simd_bitmask` by @celinval in https://github.com/model-checking/kani/pull/2677
* Add integer overflow checking for `simd_div` and `simd_rem` by @reisnera in https://github.com/model-checking/kani/pull/2645
* Bump CBMC version by @zhassan-aws in https://github.com/model-checking/kani/pull/2702
* Upgrade Rust toolchain to 2023-08-19 by @zhassan-aws in https://github.com/model-checking/kani/pull/2696

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.34.0...kani-0.35.0

## [0.34.0]

### Breaking Changes
* Change default solver to CaDiCaL by @celinval in https://github.com/model-checking/kani/pull/2557
By default, Kani will now run CBMC with CaDiCaL, since this solver has outperformed Minisat in most of our benchmarks.
User's should still be able to select Minisat (or a different solver) either by using `#[solver]` harness attribute,
or by passing `--solver=<SOLVER>` command line option.

### What's Changed

* Allow specifying the scheduling strategy in #[kani_proof] for async functions by @fzaiser in https://github.com/model-checking/kani/pull/1661
* Support for stubbing out foreign functions by @feliperodri in https://github.com/model-checking/kani/pull/2658
* Coverage reporting without a need for cbmc-viewer by @adpaco-aws in https://github.com/model-checking/kani/pull/2609
* Add support to array-based SIMD by @celinval in https://github.com/model-checking/kani/pull/2633
* Add unchecked/SIMD bitshift checks and disable CBMC flag by @reisnera in https://github.com/model-checking/kani/pull/2630
* Fix codegen of constant byte slices to address spurious verification failures by @zhassan in https://github.com/model-checking/kani/pull/2663
* Bump CBMC to v5.89.0 by @remi-delmas-3000 in https://github.com/model-checking/kani/pull/2662
* Update Rust toolchain to nightly 2023-08-04 by @remi-delmas-3000 in https://github.com/model-checking/kani/pull/2661

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.33.0...kani-0.34.0

## [0.33.0]

### What's Changed
* Add support for sysconf by @feliperodri in https://github.com/model-checking/kani/pull/2557
* Print Kani version by @adpaco-aws in https://github.com/model-checking/kani/pull/2619
* Upgrade Rust toolchain to nightly-2023-07-01 by @qinheping in https://github.com/model-checking/kani/pull/2616
* Bump CBMC version to 5.88.1 by @zhassan-aws in https://github.com/model-checking/kani/pull/2623

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.32.0...kani-0.33.0

## [0.32.0]

### What's Changed

* Add kani::spawn and an executor to the Kani library by @fzaiser in https://github.com/model-checking/kani/pull/1659
* Add "kani" configuration key to enable conditional compilation in build scripts by @celinval in https://github.com/model-checking/kani/pull/2297
* Adds posix_memalign to the list of supported builtins by @feliperodri in https://github.com/model-checking/kani/pull/2601
* Upgrade rust toolchain to nightly-2023-06-20 by @celinval in https://github.com/model-checking/kani/pull/2551
* Update rust toolchain to 2023-06-22 by @celinval in https://github.com/model-checking/kani/pull/2588
* Automatic toolchain upgrade to nightly-2023-06-24 by @github-actions in https://github.com/model-checking/kani/pull/2600
* Bump CBMC version to 5.87.0 by @adpaco-aws in https://github.com/model-checking/kani/pull/2598

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.31.0...kani-0.32.0

## [0.31.0]

### What's Changed
* Add `--exact` flag by @jaisnan in https://github.com/model-checking/kani/pull/2527
* Build the verification libraries using Kani compiler by @celinval in https://github.com/model-checking/kani/pull/2534
* Verify all Kani attributes in all crate items upfront by @celinval in https://github.com/model-checking/kani/pull/2536
* Update README.md - fix link locations for badge images in markdown by @phayes in https://github.com/model-checking/kani/pull/2537
* Bump CBMC version to 5.86.0 by @zhassan-aws in https://github.com/model-checking/kani/pull/2561

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.30.0...kani-0.31.0

## [0.30.0]

### What's Changed
* Remove --harness requirement from stubbing by @celinval in https://github.com/model-checking/kani/pull/2495
* Add target selection for cargo kani by @celinval in https://github.com/model-checking/kani/pull/2507
* Generate Multiple playback harnesses when multiple crashes exist in a single harness. by @YoshikiTakashima in https://github.com/model-checking/kani/pull/2496
* Escape Zero-size types in playback by @YoshikiTakashima in https://github.com/model-checking/kani/pull/2508
* Do not crash when `rustfmt` fails. by @YoshikiTakashima in https://github.com/model-checking/kani/pull/2511
* De-duplicate same input injections for the same harness. by @YoshikiTakashima in https://github.com/model-checking/kani/pull/2513
* Update Cbmc version  by @celinval in https://github.com/model-checking/kani/pull/2512
* Upgrade rust toolchain to 2023-04-30 by @zhassan-aws in https://github.com/model-checking/kani/pull/2456

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.29.0...kani-0.30.0


## [0.29.0]

### Major Changes
* Create a playback command to make it easier to run Kani generated tests ([pull request](https://github.com/model-checking/kani/pull/2464) by @celinval)

### What Else has Changed
* Fix symtab json file removal and reduce regression scope ([pull request](https://github.com/model-checking/kani/pull/2447) by @celinval)
* Fix regression on concrete playback inplace ([pull request](https://github.com/model-checking/kani/pull/2454) by @celinval)
* Fix static variable initialization when they have the same value ([pull request](https://github.com/model-checking/kani/pull/2469) by @celinval)
* Improve assess and regression time ([pull request](https://github.com/model-checking/kani/pull/2478) by @celinval)
* Fix playback with build scripts ([pull request](https://github.com/model-checking/kani/pull/2477) by @celinval)
* Delay printing playback harness until after verification status ([pull request](https://github.com/model-checking/kani/pull/2480) by @YoshikiTakashima)
* Update rust toolchain to 2023-04-29 ([pull request](https://github.com/model-checking/kani/pull/2452) by @zhassan-aws)
* Bump CBMC version to 5.84.0 ([pull request](https://github.com/model-checking/kani/pull/2483) by @tautschn)

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.28.0...kani-0.29.0

## [0.28.0]

### Breaking Changes
* The unstable `--c-lib` option now requires `-Z c-ffi` to enable C-FFI support by @celinval in https://github.com/model-checking/kani/pull/2425

### What's Changed
* Enforce unstable APIs can only be used if the related feature is enabled by @celinval in https://github.com/model-checking/kani/pull/2386
* Get rid of the legacy mode by @celinval in https://github.com/model-checking/kani/pull/2427
* Limit FFI calls by default to explicitly supported ones by @celinval in https://github.com/model-checking/kani/pull/2428
* Fix the order of operands for generator structs by @zhassan-aws in https://github.com/model-checking/kani/pull/2436
* Add a few options to dump the reachability graph (debug only) by @celinval in https://github.com/model-checking/kani/pull/2433
* Perform reachability analysis on a per-harness basis by @celinval in https://github.com/model-checking/kani/pull/2439
* Bump CBMC version to 5.83.0 by @zhassan-aws in https://github.com/model-checking/kani/pull/2441
* Upgrade the toolchain to nightly-2023-04-16  by @celinval in https://github.com/model-checking/kani/pull/2406

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.27.0...kani-0.28.0

## [0.27.0]

### What's Changed

* Allow excluding packages from verification with `--exclude` by @adpaco-aws in https://github.com/model-checking/kani/pull/2399
* Add size_of annotation to help CBMC's allocator by @tautschnig in https://github.com/model-checking/kani/pull/2395
* Implement `kani::Arbitrary` for `Box<T>` by @adpaco-aws in https://github.com/model-checking/kani/pull/2404
* Use optimized overflow operation everywhere by @celinval in https://github.com/model-checking/kani/pull/2405
* Print compilation stats in verbose mode by @celinval in https://github.com/model-checking/kani/pull/2420
* Bump CBMC version to 5.82.0 by @adpaco-aws in https://github.com/model-checking/kani/pull/2417

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.26.0...kani-0.27.0

## [0.26.0]

### What's Changed

* The Kani reference now includes an ["Attributes"](https://model-checking.github.io/kani/reference/attributes.html) section that describes each of the attributes available in Kani ([pull request](https://github.com/model-checking/kani/pull/2359) by @adpaco-aws)
* Users' choice of SAT solver, specified by the `solver` attribute, is now propagated to the loop-contract synthesizer ([pull request](https://github.com/model-checking/kani/pull/2320) by @qinheping)
* Unit tests generated by the concrete playback feature now compile correctly when using `RUSTFLAGS="--cfg=kani"` ([pull request](https://github.com/model-checking/kani/pull/2353) by @jaisnan)
* The Rust toolchain is updated to 2023-02-18 ([pull request](https://github.com/model-checking/kani/pull/2384) by @tautschnig)

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.25.0...kani-0.26.0


## [0.25.0]

### What's Changed

* Add implementation for the `#[kani::should_panic]` attribute by @adpaco-aws in https://github.com/model-checking/kani/pull/2315
* Upgrade Rust toolchain to nightly-2023-02-04 by @tautschnig in https://github.com/model-checking/kani/pull/2324
* Bump CBMC version to 5.80.0 by @zhassan-aws in https://github.com/model-checking/kani/pull/2336

**Full Changelog**: https://github.com/model-checking/kani/compare/kani-0.24.0...kani-0.25.0

## [0.23.0]

### Breaking Changes

- Remove the second parameter in the `kani::any_where` function by @zhassan-aws in #2257
We removed the second parameter in the `kani::any_where` function (`_msg: &'static str`) to make the function more ergonomic to use.
We suggest moving the explanation for why the assumption is introduced into a comment.
For example, the following code:
```rust
    let len: usize = kani::any_where(|x| *x < 5, "Restrict the length to a value less than 5");
```
should be replaced by:
```rust
    // Restrict the length to a value less than 5
    let len: usize = kani::any_where(|x| *x < 5);
```

### Major Changes

- Enable the build cache to avoid recompiling crates that haven't changed, and introduce `--force-build` option to compile all crates from scratch by @celinval in #2232.
