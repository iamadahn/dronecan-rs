# canard-sys

Low-level, `no_std` FFI bindings to the DroneCAN [libcanard] core
(`canard.c` / `canard.h`). This is a thin `-sys` crate: it compiles the
vendored C for the Cargo target and exposes the auto-generated bindings
verbatim (`canardInit`, `CanardInstance`, `CANARD_OK`, …). Raw functions are
unsafe; use `dronecan-core` for the safe transport API.

## Requirements

- A C cross compiler for the target (e.g. `arm-none-eabi-gcc` for
  `thumbv*-none-eabi`) — `cc` selects it from the Cargo `TARGET`.
- `libclang` (for `bindgen`) on the host.
- GNU binutils (`nm`) from the target C compiler toolchain for ABI verification.
- For a manual Git clone, the `libcanard` submodule: `git submodule update --init`.
  Cargo Git dependencies fetch it automatically; crates.io archives include
  the required C sources and need no Git checkout.

## Features

libcanard build-configuration macros, exposed as features (all off by default =
classic CAN 2.0B, single interface). They change struct ABI, so they are applied
identically to the C compilation and the bindgen pass:

| feature | macro |
|---|---|
| `canfd` | `CANARD_ENABLE_CANFD=1` |
| `multi-iface` | `CANARD_MULTI_IFACE=1` |
| `deadline` | `CANARD_ENABLE_DEADLINE=1` |
| `tao-option` | `CANARD_ENABLE_TAO_OPTION=1` |
| `asserts` | `CANARD_ENABLE_ASSERTS` (needs a libc assert handler) |
| `allocate-sem` | `CANARD_ALLOCATE_SEM=1` |

## ABI verification

The target C compiler emits arrays whose ELF symbol sizes encode `sizeof`,
`_Alignof` and non-bitfield `offsetof` measurements. Its companion `nm` reads
those symbols without executing target code. Generated Rust const assertions
compare the layouts, including enum sizes and optional ABI fields. C and bindgen
use `-fno-short-enums` to agree on 32-bit enum representation on ARM. A missing measurement fails
the build instead of skipping a check. Supported object targets are Linux and
Cortex-M ELF; no compiler-diagnostic parsing is used.

`asserts` needs the target assertion handler. `allocate-sem` needs implementations
of `canard_allocate_sem_take`/`canard_allocate_sem_give`; neither feature provides
thread safety for the complete protocol instance by itself.

## For downstream `-sys` crates

Via its `links = "canard"` key, the build script exports:

- `DEP_CANARD_INCLUDE` — path to the directory containing `canard.h`,
- `DEP_CANARD_DEFINES` — the exact `KEY=VALUE` defines libcanard was built with,
- `DEP_CANARD_CFLAGS` — ABI compiler flags, currently `-fno-short-enums`,

so a crate compiling C against libcanard (e.g. `dronecan-dsdl-sys`) can share the
same ABI.

## License

This crate is `MIT OR Apache-2.0`. The bundled libcanard sources are MIT
(© UAVCAN Team) — see `libcanard/LICENSE`.

[libcanard]: https://github.com/dronecan/libcanard
