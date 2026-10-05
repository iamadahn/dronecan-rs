//! Generated FFI bindings for DroneCAN DSDL message (de)serialization.
//!
//! Default builds compile vendored C produced by [dronecan_dsdlc]; disabling
//! `vendored` regenerates it at build time from the selected DSDL namespaces.
//! Each message type comes with `_encode` / `_decode`
//! functions that operate on [`canard_sys`] transfer buffers — the Canard types
//! in their signatures are re-exported from `canard_sys`, not duplicated here.
//!
//! [dronecan_dsdlc]: https://github.com/dronecan/dronecan_dsdlc
#![no_std]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::all)]
// Empty DSDL messages generate zero-sized C structs used across `extern` blocks;
// Rust flags those as not-FFI-safe, but they're benign (no data to pass).
#![allow(improper_ctypes)]

pub use canard_sys;

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
