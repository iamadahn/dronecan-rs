# dronecan-core

Safe `no_std`, heap-free DroneCAN v0 transport over `canard-sys`, with typed
`NodeStatus` and `GetNodeInfo` codecs. Default builds use vendored DSDL C.

## Installation

```toml
[dependencies]
dronecan-core = { git = "https://github.com/iamadahn/dronecan-rs", tag = "v0.1.0" }
```

After publication to crates.io, use `dronecan-core = "0.1.0"` instead.
Requires Rust 1.85+, host libclang, a target C compiler and GNU `nm` (Linux
or Cortex-M ELF). For Cortex-M0+, install the `thumbv6m-none-eabi` Rust
target and the `arm-none-eabi-gcc`/binutils/newlib toolchain on the build host.
This crate has no dependency on Embassy or a specific HAL.

The crate's Rust code is MIT OR Apache-2.0. Its native dependencies retain
the licenses listed in THIRD-PARTY-NOTICES.md, including the upstream DSDL
generator/templates distributed by dronecan-dsdl-sys.

## Ownership and lifetime

`Node` exclusively borrows an `Arena<N>` and a subscription slice. The arena
cannot move, be reused or be dropped while the node exists. Moving the node is
safe: libcanard's persistent pointers address the borrowed arena, not the Rust
node. RX callback context exists only during the synchronous `receive` call.
No application callback runs inside C, and returned transfers own their bytes.

A node is intentionally neither Send nor Sync. Operate it from one task. For an
async driver, call `try_transmit` with a synchronous, nonblocking enqueue closure,
then wait for RX, TX capacity or maintenance outside the core. On enqueue failure
the head stays queued. Never await inside the enqueue closure.

```rust
use dronecan_core::{Arena, MEM_BLOCK_SIZE, Node, NodeId, NodeInfo, NodeStatus, Subscription};
let mut arena = Arena::<{ MEM_BLOCK_SIZE * 256 }>::new();
let subscriptions = [Subscription::request(NodeInfo::SERVICE)];
let mut node: Node<'_, { MEM_BLOCK_SIZE * 256 }> =
    Node::new(&mut arena, NodeId::new(69).unwrap(), &subscriptions).unwrap();
node.broadcast_status(NodeStatus::operational(0), 1).unwrap();
```

The arena's lifetime and task ownership are enforced by Rust:

```compile_fail
use dronecan_core::{Arena, MEM_BLOCK_SIZE, Node, NodeId};
let mut arena = Arena::<{ MEM_BLOCK_SIZE * 256 }>::new();
let mut node: Node<'_, { MEM_BLOCK_SIZE * 256 }> =
    Node::new(&mut arena, NodeId::new(69).unwrap(), &[]).unwrap();
drop(arena); // cannot move storage while the node retains C pointers
node.cleanup(1).unwrap();
```

```compile_fail
use dronecan_core::{Arena, MEM_BLOCK_SIZE, Node, NodeId};
fn requires_send<T: Send>(_: T) {}
let mut arena = Arena::<{ MEM_BLOCK_SIZE * 256 }>::new();
let node: Node<'_, { MEM_BLOCK_SIZE * 256 }> =
    Node::new(&mut arena, NodeId::new(69).unwrap(), &[]).unwrap();
requires_send(node); // protocol state cannot be shared across task threads
```

## Bounds and timing

`Node<'a, N, MAX, SESSIONS>` defaults to a 512-byte owned RX payload and 16 TX
session descriptors. Choose these compile-time capacities for the application.
A full TX session table returns `TooManyTxSessions`; existing transfer IDs remain
intact. TX payloads are limited to the native 10-bit length (1023 bytes), including
CAN FD DLC padding. Generic FD payloads may include trailing zero padding.
Typed DSDL messages use explicit array lengths for FD and TAO for classic CAN.

Pass nondecreasing processing timestamps in microseconds to RX, TX and cleanup.
`cleanup` should run at least once per second. These timestamps are for transport
timeouts, not synchronized network time. Track `pool_statistics()` under expected
bus load; capacity, current use and peak use are measured in native memory blocks.
`Counters` reports filtering, RX/TX errors, CRC failures, OOM and accepted TX
frames. TX acceptance means ownership passed to the driver, not a bus ACK.

## Configuration

Use validated `NodeId`, `Priority`, `NodeName` and `HardwareVersion` constructors.
`NodeConfig::info(status)` builds a GetNodeInfo report without modifying static
identity/version data. RX preserves a remote undefined UID; local configuration
should always use a nonzero hardware identity.

Subscriptions match both transfer kind and ID and supply the correct signature.
Raw `MessageType`/`ServiceType` values and owned payloads support custom DSDL
without exposing native pointers. The typed decoders check message kind/ID.

## Features

`canfd`, `deadline`, `multi-iface`, `tao-option`, `vendored` (default). The wrapper
reads the actual native configuration after Cargo feature unification. `canfd`
enables FD capability; `set_can_fd(true)` selects FD for a particular node's TX.
Classic framing remains usable in the same build. Native deadlines are one second
from enqueue, and cleanup expires overdue frames. The wrapper uses interface 0
and TX mask 1; redundant physical interfaces are not implemented.

Low-level `asserts` and `allocate-sem` require application-provided C hooks.
