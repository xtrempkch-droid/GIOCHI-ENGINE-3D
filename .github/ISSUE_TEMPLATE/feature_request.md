---
name: Feature request
about: Propose an addition or change to the engine
title: "[feat] "
labels: ["enhancement"]
---

## Problem

<!-- What can you not do today? Describe the use case, not the solution. -->

## Proposed API

<!--
The public surface is C-shaped (see docs/C_ABI_CONTRACT.md): `#[repr(C)]`
structs with `_reserved` padding, opaque `u64` handles, `i32` error codes. Show
the shape you have in mind.

```c
typedef struct GiochiThing {
    uint64_t handle;
    float    x, y;
    uint32_t _reserved[4];
} GiochiThing;

int32_t giochi_thing_create(GiochiThing* out);
```
-->

## Alternatives considered

<!-- Other designs, and why they are worse. -->

## Scope check

The current version is **2D only**. Tick what applies:

- [ ] This fits the existing 2D scope.
- [ ] This is 2D but needs a new subsystem (say which).
- [ ] This needs 3D — deliberately out of scope for the whole 0.1 line (see
      [docs/DECISIONS.md](../../docs/DECISIONS.md), ADR-005).
