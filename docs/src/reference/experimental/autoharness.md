# Automatic Harness Generation

Recall the harness for `estimate_size` that we wrote in [First Steps](../../tutorial-first-steps.md):
```rust
{{#include ../../tutorial/first-steps-v1/src/lib.rs:kani}}
```

This harness first declares a local variable `x` using `kani::any()`, then calls `estimate_size` with argument `x`.
Many proof harnesses follow this predictable format—to verify a function `foo`, we create arbitrary values for each of `foo`'s arguments, then call `foo` on those arguments.

The `autoharness` subcommand leverages this observation to automatically generate harnesses and run Kani against them.
Kani scans the crate for functions whose arguments all implement the `kani::Arbitrary` trait, generates harnesses for them, then runs them.
These harnesses are internal to Kani—i.e., Kani does not make any changes to your source code.

## Usage
Run either:
```
# cargo kani autoharness -Z autoharness
```
or
```
# kani autoharness -Z autoharness <FILE>
```

If Kani detects that all of a function `foo`'s arguments implement `kani::Arbitrary`, it will generate and run a `#[kani::proof]` harness, which prints:

```
Autoharness: Checking function foo against all possible inputs...
<VERIFICATION RESULTS>
```

However, if Kani detects that `foo` has a [contract](./contracts.md), it will instead generate a `#[kani::proof_for_contract]` harness and verify the contract:
```
Autoharness: Checking function foo's contract against all possible inputs...
<VERIFICATION RESULTS>
```

Kani generates and runs these harnesses internally—the user only sees the verification results.

The `autoharness` subcommand has options `--include-function` and `--exclude-function` to include and exclude particular functions.
These flags look for partial matches against the fully qualified name of a function.

For example, if a module `my_module` has many functions, but we are only interested in `my_module::foo` and `my_module::bar`, we can run:
```
cargo run autoharness -Z autoharness --include-function foo --include-function bar
```
To exclude `my_module` entirely, run:
```
cargo run autoharness -Z autoharness --exclude-function my_module
```

## Example
Using the `estimate_size` example from [First Steps](../../tutorial-first-steps.md) again:
```rust
{{#include ../../tutorial/first-steps-v1/src/lib.rs:code}}
```

We get:

```
# cargo kani autoharness -Z autoharness
Autoharness: Checking function estimate_size against all possible inputs...
RESULTS:
Check 3: estimate_size.assertion.1
         - Status: FAILURE
         - Description: "Oh no, a failing corner case!"
[...]

Verification failed for - estimate_size
Complete - 0 successfully verified functions, 1 failures, 1 total.
```

## Request for comments
This feature is experimental and is therefore subject to change.
If you have ideas for improving the user experience of this feature,
please add them to [this GitHub issue](https://github.com/model-checking/kani/issues/3832).

## Limitations
### Arguments Implementing Arbitrary
Kani will only generate an automatic harness for a function if it can determine that all of the function's arguments implement Arbitrary.
It does not attempt to derive/implement Arbitrary for any types, even if those types could implement Arbitrary.
For example, imagine a user defines `struct MyStruct { x: u8, y: u8}`, but does not derive or implement Arbitrary for `MyStruct`.
Kani does not attempt to add such derivations itself, so it will not generate a harness for a function that takes `MyStruct` as input.

### Generic Functions
The current implementation does not generate harnesses for generic functions.
For example, given:
```rust
fn foo<T: Eq>(x: T, y: T) {
    if x == y {
        panic!("x and y are equal");
    }
}
```
Kani would report that no functions were eligible for automatic harness generation.

If, however, some caller of `foo` is eligible for an automatic harness, then a monomorphized version of `foo` may still be reachable during verification.
For instance, if we add `main`:
```rust
fn main() {
    let x: u8 = 2;
    let y: u8 = 2;
    foo(x, y);
}
```
and run the autoharness subcommand, we get:
```
Autoharness: Checking function main against all possible inputs...

Failed Checks: x and y are equal
 File: "src/lib.rs", line 3, in foo::<u8>

VERIFICATION:- FAILED
```

### Loop Contracts
If a function contains a loop with a loop contract, Kani will detect the presence of a loop contract and verify that contract.
If, however, the loop does not have a contract, then there is currently no way to specify an unwinding bound for the function, meaning that Kani may hang as it tries to unwind the loop.
We recommend using the `--exclude-function` option to exclude any functions that have this issue (or `--harness-timeout` to bail after attempting verification for some amount of time).