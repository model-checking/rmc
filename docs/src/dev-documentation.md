# Developer documentation

Kani is an open source project open to external contributions.

The easiest way to contribute is to [report any
issue](https://github.com/model-checking/kani/issues/new/choose) you encounter
while using the tool. If you want to contribute to its development,
we recommend looking into [these issues](https://github.com/model-checking/kani/contribute).

In this chapter, we provide documentation that might be helpful for Kani
developers (including external contributors):
 1. [Coding conventions](./conventions.md).
 2. [Useful command-line instructions for Kani/CBMC/Git](./cheat-sheets.md).
 3. [Development setup recommendations for working with `cbmc`](./cbmc-hacks.md).
 4. [Development setup recommendations for working with `rustc`](./rustc-hacks.md).
 5. [Transition to StableMIR](./stable-mir.md).
 6. [Using cargo kani assess](./dev-assess.md).
 7. [Demonic non-determinism](./demonic-nondet.md)
 8. [Guide for testing in Kani](./testing.md).
 9. [Performance comparison for Kani](./performance-comparisons.md).

> **NOTE**: The developer documentation is intended for Kani developers and not
users. At present, the project is under heavy development and some items
discussed in this documentation may stop working without notice (e.g., commands
or configurations). Therefore, we recommend users to not rely on them.
