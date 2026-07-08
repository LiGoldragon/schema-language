# Syntax redesign proposal

This file was recreated for the first deterministic schema-generic slice after the earlier local proposal was not present in this checkout.

## Current slice

- Schema source has a positional generics section between imports and input/output roots.
- Generic rows are typed data in that section. Builtin instances use closed variants such as `Vector`, `Optional`, `ScopeOf`, `Map`, `FixedBytes`, and frame generics use `(Frame [Parameters] [Variants])`.
- Type invocation moves away from parenthesized application toward dotted structural spelling: unary invocations may be flat (`Vector.Topic`), while multi-argument invocations group their argument list (`Map.(Key Value)`, `Work.(SignalInput SemaWriteOutput SemaReadOutput EffectOutcome)`). A flat chain such as `Map.Key.Value` is unary nesting and is not a two-argument map.

## Later work

TODO: consider a one-shot migrator from retired parenthesized generic declaration/application syntax to the positional generics section plus dotted invocation. That migrator is out of this slice.
