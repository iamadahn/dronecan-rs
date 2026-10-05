//! Allocation-free DroneCAN v0. Mutable libcanard state requires exclusive
//! access to a [`Node`]. Callbacks copy transfers before returning to C.
#![doc = include_str!("../README.md")]
#![no_std]

mod messages;
mod transport;
mod types;

pub use messages::{
    HardwareVersion, Health, Mode, NodeConfig, NodeInfo, NodeName, NodeStatus, SoftwareVersion,
};
pub use transport::{Arena, Counters, Node, PoolStatistics, ReceivedTransfer, TransferMetadata};
pub use types::{
    Error, Frame, MessageType, NodeId, Priority, ServiceType, Subscription, TransferKind,
};

pub const MEM_BLOCK_SIZE: usize = canard_sys::CANARD_MEM_BLOCK_SIZE as usize;
pub const CAN_FD_SUPPORTED: bool = cfg!(canard_canfd);
pub const TRANSFER_TIMEOUT_US: u64 = 2_000_000;
