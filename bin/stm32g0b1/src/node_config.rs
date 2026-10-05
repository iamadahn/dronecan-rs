use dronecan_core::{HardwareVersion, NodeConfig, NodeId, NodeName, SoftwareVersion};

pub const NODE_ID: u8 = 69;
pub const NODE_NAME: &str = "alan.battery";

pub fn config() -> NodeConfig {
    // Preserve the complete STM32 96-bit UID, with a fixed product namespace.
    // This is deterministic across boots and firmware upgrades.
    let mut unique_id = [0; 16];
    unique_id[..4].copy_from_slice(b"ALAN");
    unique_id[4..].copy_from_slice(&embassy_stm32::uid::uid());
    NodeConfig {
        id: NodeId::new(NODE_ID).expect("node ID must be 1..=127"),
        name: NodeName::new(NODE_NAME).expect("invalid DroneCAN name"),
        software: SoftwareVersion {
            major: 0,
            minor: 1,
            vcs_commit: None,
            image_crc: None,
        },
        hardware: HardwareVersion::new(0, 0, unique_id).expect("nonzero hardware UID"),
    }
}
