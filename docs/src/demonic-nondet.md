# Demonic non-determinism in CBMC and Kani

CBMC and Kani use demonic non-determinism to more efficiently prove certain
classes of program properties.
This document describes what we mean by "demonic non-determinism," how and why
CBMC and Kani use it, and what challenges arise as a consequence thereof.

## What is demonic non-determinism?

When we seek to prove that a property holds for all possible values of some
object, a counterexample demonstrates the existence of at least one such
value that violates the property.
Other properties range across all objects or expressions of a particular kind,
e.g., we want to prove that for all dereference expressions the underlying
pointer is valid.
For a counterexample to this property it suffices to find one pointer that
cannot be safely dereferenced.
We can use this fact to non-deterministically choose to only track a single
pointer across an execution via expressions of the form
`global_pointer = if nondet { new_pointer_to_track } else { global_pointer };`.
We can now use `global_pointer` in describing properties such that the property
fails if `new_pointer_to_track` is invalid and `global_pointer` equals
`new_pointer_to_track`.
Note that this is an "if" statement, not "if, and only if:" there may also be
some other invalid pointer `other_pointer` that is invalid at the time of
dereference, and the back-end solver may choose to resolve the non-determinism
such that the property fails because `global_pointer` equals `other_pointer`.
We call such use of non-determinism _demonic non-determinism_, because the
SAT or SMT-solver will resolve the non-deterministic choice such that a
counterexample is reported _if_ there exists a model such that the overall
property fails.

## Why use demonic non-determinism?

For the above example, one way to avoid the non-determinism is to use an array
or map to track all pointers rather than non-deterministically choosing to track
a single one.
The encoding thereof, however, may be much more expensive.  See
[cbmc#6506](https://github.com/diffblue/cbmc/pull/6506) for the performance
penalty for such a change from non-determinism to arrays.

## What are disadvantages of demonic non-determinism?

The consequence of this choice of encoding is that any given model provided by
the back-end SAT/SMT solver can only encode a violation of one property of the
kind that the demonic non-determinism is used for, even if multiple properties
can be violated along a single execution path.
When we now look at assumptions the problem is that these cannot accomplish what
we’d perhaps expect to get out of them: the `assume` statement with a condition of
"single global variable does not have the same value as the pointer to be
dereferenced" does not guarantee that the subsequent dereference is safe with
regard to objects having gone out of scope: the solver can choose to pick
non-deterministic values such that the address of the object having gone out of
scope has never been assigned.

As such, demonic non-determinism poses a problem in context of modular
verification, where we need to assume that the given preconditions hold.
Such assumed preconditions would not be preceded by an assertion, and we would
effectively assume that none of the pointers involved in such preconditions were
being tracked by the solver.

## Where do CBMC and Kani use demonic non-determinism?

One way we use this today is tracking objects that go out of scope.
We use a single, global variable (initialized to the null pointer) to store the
address of objects going out of scope.
Upon a pointer dereference we then check whether the pointer being dereferenced
has the same (address) value as the value held in that single global variable.
If that is true then we know a dead object (one that has gone out of scope) is
being accessed, amounting to undefined behavior, and we fail that property check.

This setup can obviously only track a single object (the one that has gone out
of scope most recently).
By adding non-determinism when assigning the address to that single global
variable (`global-ptr = nondet ? global-ptr : &object-leaving-scope;`) we leave
the choice of which object to track to the solver.
This is sound even in presence of multiple objects (and, possibly, multiple
invalid pointers), because the solver will
return at least one counterexample if there is any undefined behavior, and can
indeed be forced to return all such counterexamples.
