# Embassy DroneCAN node on STM32G0B1CB

Standalone firmware for STM32G0B1CB: 128 KiB flash, 144 KiB SRAM,
`thumbv6m-none-eabi`. Cargo MCU selection, linker memory and probe-rs runner all
use this part.

## Hardware

- 8 MHz external crystal; PLL provides 64 MHz CPU/FDCAN clock.
- FDCAN1 RX on PD0, TX on PD1.
- External CAN transceiver and suitable bus termination.
- Nominal bitrate: 1 Mbps.
- SWD programmer supported by probe-rs.

For `canfd`, use an FD-capable transceiver and peers. FD uses the same nominal
bitrate with bit-rate switching disabled; classic CAN is the default.

## Build and run

```sh
cd bin/stm32g0b1
cargo build --release --locked
cargo run --release --locked
cargo build --release --locked --features canfd
cargo build --release --locked --all-features
```

Default builds need libclang and arm-none-eabi GCC/binutils, with no Python.
`--no-default-features` switches to build-time DSDL generation and requires the
root `.venv` or `DRONECAN_DSDLC_PYTHON`.

## Node behavior and configuration

- Node ID 69, name `alan.battery`; edit `src/node_config.rs`.
- Persistent 128-bit UID: the `ALAN` product prefix plus all 12 STM32 UID bytes.
- Software/hardware versions are configured explicitly in `node_config.rs`.
- NodeStatus is broadcast every 500 ms on a fixed schedule in all configurations.
- GetNodeInfo accepts only requests for that service, preserves request transfer
  ID and priority, and encodes exactly the name's occupied bytes.
- One task owns the protocol; IRQs service bounded RX/TX driver rings.
- The driver TX ring has 8 frames, RX ring 32 frames. The pool has 256 native
  blocks (8 KiB classic, 10 KiB with deadline, 32 KiB FD).
- Cleanup runs every 500 ms. Pool/error counters are logged every 10 seconds.
- A full TX ring leaves the libcanard head queued; RX/maintenance continue.
- RTT logging uses defmt. Transport errors and CAN bus errors are counted/logged.

The RX ring is finite: sustained traffic beyond processing capacity can overflow
HAL buffers. Pool size and ring capacities should be measured on the real bus.
The template has no battery measurement/business logic yet.

## Files and features

`main.rs` starts the board and runner; `board.rs` contains clocks/pins/FDCAN;
`node_config.rs` defines identity/versions; `runner.rs` contains async scheduling.
No application code calls raw FFI.

Supported transport features: `canfd`, `tao-option`, `deadline`, `multi-iface`
(native ABI, still one physical interface), and `vendored` (default). Extra DSDL
features: `dsdl-uavcan`, `dsdl-dronecan`, `dsdl-ardupilot`, `dsdl-com`, `dsdl-cuav`,
`dsdl-mppt`. `uavcan` is always included because the node needs standard services.
Native `asserts`/`allocate-sem` hooks belong to low-level integrations and are not
exposed by this firmware.
