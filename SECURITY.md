# Security policy

## Supported versions

The engine is pre-1.0 and ships from the `main` branch. Fixes go into the next release; there
are no maintained older lines.

| Version | Supported |
|---|---|
| 0.5.x (current) | yes |
| ≤ 0.4.x | no |

## Reporting a vulnerability

**Please do not open a public issue for a vulnerability.** Use GitHub's private reporting:
*Security* → *Report a vulnerability* on the repository
(`https://github.com/xtrempkch-droid/GIOCHI-ENGINE-3D/security/advisories/new`). If that is
unavailable, open an issue that says only that you have a report and how to reach you, with no
details.

Include, if you can:

- the version (`git rev-parse --short HEAD`, plus `giochi-racer --version` or
  `giochi-editor --version`, or `GIOCHI_ABI_VERSION` from `giochi.h`);
- whether it reproduces on `x86_64` or only on `aarch64`/KMSDRM;
- the smallest program or `.scene`/`.lua` file that shows it;
- what an attacker gains: a crash, code execution, a file written outside the intended place.

Expect an acknowledgement within a week and a fix or a mitigation plan in the next release.

## What is in scope

The attack surface is larger than a typical library's, because the engine loads files and
crosses an FFI boundary:

| Surface | Examples |
|---|---|
| **The C ABI** (`giochi-engine-capi`) | null pointers, handles that were already destroyed, mismatched `GIOCHI_ABI_VERSION` |
| **Asset loaders** | OBJ, glTF/GLB, PNG, shaders, `.scene` JSON, `.lua` |
| **Lua** (`giochi-engine-lua`) | a script escaping its sandbox, a panic that unwinds across the FFI |
| **Networking** (`giochi-net`) | malformed datagrams, a datagram that makes the host allocate |
| **Editor** (`giochi-editor`) | a project or scene file that escapes the project directory |

## What is out of scope

- A malformed **asset** that crashes a game the user chose to run with their own file:
  report it, and it is a bug worth fixing, but it is not a privilege boundary.
- Denial of service against a host that accepted a peer it did not authenticate — the
  protocol is LAN-only and host-authoritative by design, and it has no authentication yet
  ([docs/REDE.md](docs/REDE.md)).
- Anything that requires the attacker to already run code on the device.

## Practices in the repository

- `unsafe` is confined to the FFI and GL layers and carries `// SAFETY:` notes; the core crate
  is `#![forbid(unsafe_code)]`.
- Every public type is `#[repr(C)]` with `_reserved` fields, and `tests/c_shape.rs` enforces it
  mechanically — layout drift is a compile-and-test failure rather than a memory bug.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check` are gates,
  as are shellcheck and a YAML parse of the workflows.
- Dependencies are reviewed for MSRV and cross-compile cost before being added, and Dependabot
  is enabled.
