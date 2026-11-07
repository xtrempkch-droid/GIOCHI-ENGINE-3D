## What this changes

<!-- One paragraph. What behaviour is different after this PR? -->

## Why

<!-- The problem it solves. Link the issue: Closes #123 -->

## C ABI impact

The public surface is C-shaped (see [docs/C_ABI_CONTRACT.md](../docs/C_ABI_CONTRACT.md)).
Tick exactly one:

- [ ] No public type or function changes (internal refactor, docs, tests, CI).
- [ ] Adds a new public item — `docs/C_ABI_CONTRACT.md` updated, `_reserved`
      present on any new struct.
- [ ] Changes an existing public item — **breaking**: ABI version bumped and
      migration notes added to `CHANGELOG.md`.

## Checklist

- [ ] `./scripts/gates.sh` passes (fmt, clippy `-D warnings`, build, test, doc).
- [ ] `./scripts/check-aarch64.sh` passes (or the change cannot affect ARM64).
- [ ] New behaviour has a test. Bugs get a regression test that fails without
      the fix.
- [ ] Documentation updated: `README.md`, `docs/`, and the doc comment on any
      touched public item.
- [ ] `PROGRESS.md` updated if this completes a phase.
- [ ] No `unsafe` added without a `// SAFETY:` comment saying why it is sound.
