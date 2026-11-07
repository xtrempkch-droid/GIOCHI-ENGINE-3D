---
name: Bug report
about: Something in the engine does not behave as documented
title: "[bug] "
labels: ["bug"]
---

## What happened

<!-- A clear description of the observed behaviour. -->

## What you expected

<!-- What the docs (API_GUIDE.md, C_ABI_CONTRACT.md) say should happen. -->

## Reproduction

```bash
# Minimal sequence that triggers it.
```

If you can, reduce it to a small Rust snippet against `giochi-engine`:

```rust
// code
```

## Environment

| Item | Value |
|---|---|
| Target | `x86_64-unknown-linux-gnu` / `aarch64-unknown-linux-gnu` |
| Rust version | `rustc --version` |
| SDL video driver | `SDL_VIDEODRIVER` (empty = auto) |
| Display / KMSDRM | desktop session / bare console / headless |
| `giochi-engine` version | `0.1.0` |

## Logs

<!--
Re-run with logging enabled:

    RUST_LOG=debug cargo run -p giochi-demo

Paste the output, plus the value of `giochi_get_last_error()` if you went
through the C ABI.
-->

```
```

## Checklist

- [ ] I reproduced this on the latest `main`.
- [ ] I searched the existing issues.
- [ ] This is not a question about how to use the engine (see
      [docs/API_GUIDE.md](../../docs/API_GUIDE.md)).
