# Changelog

## 0.1.0

Initial release of the Rust library workspace:

- Safe `no_std` transport with a borrowed aligned arena, owned RX payloads,
  bounded transfer-ID sessions, TX backpressure and transport counters.
- Typed NodeStatus and GetNodeInfo, validated node identity and configuration.
- Native classic CAN, optional CAN FD, deadlines, TAO and multi-interface ABI.
  The safe wrapper uses one physical interface.
- Raw libcanard and codecs for six standard/vendor DSDL namespaces, reproducible
  vendored C, and optional Python source generation.
- Target C/Rust ABI assertions, Linux host protocol tests and STM32G0B1 Embassy
  firmware builds, including Cortex-M0+ enum ABI verification.
- Self-contained Cargo packages, license texts and upstream notices.

Embassy and the STM32 HAL are dependencies of the firmware example only.
