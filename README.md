# dronecan-rs

Allocation-free DroneCAN v0 using the vendored C libcanard transport. This is
DroneCAN/UAVCAN v0, not Cyphal/UAVCAN v1.

## Use in a project

The public repository is https://github.com/iamadahn/dronecan-rs. Add the safe
core to your application (Embassy, another runtime, or a synchronous loop):

```toml
[dependencies]
dronecan-core = { git = "https://github.com/iamadahn/dronecan-rs", tag = "v0.1.0", features = ["deadline"] }
```

This requires the repository and `v0.1.0` tag to have been pushed. For a development
revision, replace `tag` with `rev` containing a full commit hash. Cargo fetches
the workspace crate and its Git submodules. No Python is needed with the defaults.

After the crates.io release, the equivalent dependency is:

```toml
[dependencies]
dronecan-core = { version = "0.1.0", features = ["deadline"] }
```

For extra generated message types, add `dronecan-dsdl-sys` from the same Git tag
or registry version and select namespaces, e.g. `features = ["ardupilot"]`.
The build host needs Rust 1.85+, libclang, a target C compiler and GNU `nm`.
Supported object targets are Linux and Cortex-M ELF.

## Architecture

| Crate | Responsibility |
| --- | --- |
| `canard-sys` | Raw libcanard FFI, native build configuration and ABI checks. |
| `dronecan-dsdl-sys` | Generated C DSDL codecs and raw Rust bindings. |
| `dronecan-core` | Safe transport, owned RX transfers, typed standard node messages. |
| `bin/stm32g0b1` | Standalone STM32G0B1CB firmware with one Embassy protocol owner. |

The core has no HAL/runtime dependency or heap allocation. Its `Node` exclusively
borrows an aligned arena. Native callbacks copy reassembled payloads into owned
Rust values and return before application code runs. Transfer IDs are independent
for each broadcast type and each service/destination pair.

TX uses a synchronous `try_transmit` handoff: the libcanard queue head is removed
only after the driver accepts the copied frame. A full driver queue leaves it
queued. No await holds a libcanard frame pointer. The firmware selects RX, TX
capacity and a fixed maintenance timer, so a blocked TX cannot block RX/cleanup.

## Build

Initialize the libcanard submodule, install Rust and its MCU target, a C compiler
with GNU binutils (`nm`), and host libclang. Cortex-M additionally needs
`arm-none-eabi-gcc` and its binutils.

```sh
git submodule update --init --recursive
rustup target add thumbv6m-none-eabi
cargo test --workspace --locked
cd bin/stm32g0b1
cargo build --release --locked
cargo run --release --locked
```

Firmware is a separate workspace; root workspace checks do not build it.
Default builds use checked-in DSDL C and need no Python. The firmware's default
namespace is only `uavcan`. Extra DSDL namespaces are optional firmware features.

## DSDL maintenance

Generated C is in `dronecan-dsdl-sys/vendored`. The manifest records submodule
revisions, pinned Python package versions and file hashes. Update definitions and
regenerate deliberately:

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
.venv/bin/python tools/generate_dsdl.py
.venv/bin/python tools/generate_dsdl.py --check
```

For experimental DSDL changes, disable the default `vendored` feature:

```sh
cargo test -p dronecan-core --no-default-features
# Override interpreter discovery if needed:
DRONECAN_DSDLC_PYTHON=/path/to/python cargo test -p dronecan-core --no-default-features
```

Python is required only for regeneration or this source-generation mode.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p dronecan-core --all-features --locked
cargo check --workspace --all-features --locked
cd bin/stm32g0b1
cargo fmt --all -- --check
cargo clippy --all-features --locked -- -D warnings
cargo build --release --all-features --locked
```

The host regression suite exchanges real libcanard frames: CRC, service routing,
GetNodeInfo/NodeStatus codecs, transfer-ID wrap, pool exhaustion/recovery, TX
backpressure, CAN FD padding and TX deadlines. CI also builds the firmware
separately for classic CAN, TAO, deadline, native multi-interface ABI and CAN FD.

`asserts` and `allocate-sem` remain low-level `canard-sys` integration options;
consumers must provide the native assertion/semaphore hooks. They are not
firmware features. The high-level wrapper uses one physical interface, including
when the native `multi-iface` ABI is enabled.

## Documentation and licensing

See [core API and ownership](dronecan-core/README.md),
[firmware hardware/configuration](bin/stm32g0b1/README.md), and the crate READMEs.
See [publishing instructions](docs/maintainers/releasing.md) and [release changes](CHANGELOG.md).
Rust sources authored here use `MIT OR Apache-2.0` with both license texts included.
libcanard and DSDL retain their upstream MIT notices. The DSDL package also
ships the upstream GPLv3-or-later generator and templates, so its package license
expression includes GPL-3.0-or-later. See [third-party notices](THIRD-PARTY-NOTICES.md)
for component licenses and the provenance of generated codecs.
