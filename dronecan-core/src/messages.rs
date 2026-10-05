use crate::{
    Error, MessageType, Node, NodeId, Priority, ReceivedTransfer, ServiceType, TransferKind,
};
use dronecan_dsdl_sys as dsdl;

pub const NODE_STATUS: MessageType = MessageType {
    id: dsdl::UAVCAN_PROTOCOL_NODESTATUS_ID,
    signature: dsdl::UAVCAN_PROTOCOL_NODESTATUS_SIGNATURE,
};
pub const GET_NODE_INFO: ServiceType = ServiceType {
    id: dsdl::UAVCAN_PROTOCOL_GETNODEINFO_ID as u8,
    signature: dsdl::UAVCAN_PROTOCOL_GETNODEINFO_SIGNATURE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Health {
    Ok = 0,
    Warning = 1,
    Error = 2,
    Critical = 3,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    Operational = 0,
    Initialization = 1,
    Maintenance = 2,
    SoftwareUpdate = 3,
    Offline = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeStatus {
    pub uptime_sec: u32,
    pub health: Health,
    pub mode: Mode,
    pub vendor_specific_status_code: u16,
}
impl NodeStatus {
    pub const TYPE: MessageType = NODE_STATUS;
    pub const fn operational(uptime_sec: u32) -> Self {
        Self {
            uptime_sec,
            health: Health::Ok,
            mode: Mode::Operational,
            vendor_specific_status_code: 0,
        }
    }
    fn native(self) -> dsdl::uavcan_protocol_NodeStatus {
        dsdl::uavcan_protocol_NodeStatus {
            uptime_sec: self.uptime_sec,
            health: self.health as u8,
            mode: self.mode as u8,
            sub_mode: 0,
            vendor_specific_status_code: self.vendor_specific_status_code,
        }
    }
    fn from_native(status: dsdl::uavcan_protocol_NodeStatus) -> Result<Self, Error> {
        let health = match status.health {
            0 => Health::Ok,
            1 => Health::Warning,
            2 => Health::Error,
            3 => Health::Critical,
            _ => return Err(Error::Decode),
        };
        let mode = match status.mode {
            0 => Mode::Operational,
            1 => Mode::Initialization,
            2 => Mode::Maintenance,
            3 => Mode::SoftwareUpdate,
            7 => Mode::Offline,
            _ => return Err(Error::Decode),
        };
        Ok(Self {
            uptime_sec: status.uptime_sec,
            health,
            mode,
            vendor_specific_status_code: status.vendor_specific_status_code,
        })
    }
    pub fn decode<const MAX: usize>(transfer: &ReceivedTransfer<MAX>) -> Result<Self, Error> {
        if transfer.metadata.kind != TransferKind::Broadcast
            || transfer.metadata.data_type_id != NODE_STATUS.id
        {
            return Err(Error::InvalidTransfer);
        }
        let native = transfer.native();
        let mut status = unsafe { core::mem::zeroed() };
        if unsafe { dsdl::_uavcan_protocol_NodeStatus_decode(&native, &mut status) } {
            return Err(Error::Decode);
        }
        Self::from_native(status)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeName {
    data: [u8; 80],
    len: u8,
}
impl NodeName {
    pub fn new(name: &str) -> Result<Self, Error> {
        if name.is_empty()
            || name.len() > 80
            || !name.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
            })
        {
            return Err(Error::InvalidName);
        }
        let mut result = Self {
            data: [0; 80],
            len: name.len() as u8,
        };
        result.data[..name.len()].copy_from_slice(name.as_bytes());
        Ok(result)
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len as usize]).expect("validated ASCII name")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoftwareVersion {
    pub major: u8,
    pub minor: u8,
    pub vcs_commit: Option<u32>,
    pub image_crc: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardwareVersion {
    pub major: u8,
    pub minor: u8,
    unique_id: [u8; 16],
    certificate: [u8; 255],
    certificate_len: u8,
}
impl HardwareVersion {
    pub fn new(major: u8, minor: u8, unique_id: [u8; 16]) -> Result<Self, Error> {
        if unique_id == [0; 16] {
            return Err(Error::InvalidUniqueId);
        }
        Ok(Self {
            major,
            minor,
            unique_id,
            certificate: [0; 255],
            certificate_len: 0,
        })
    }
    pub fn unique_id(&self) -> &[u8; 16] {
        &self.unique_id
    }
    pub fn certificate(&self) -> &[u8] {
        &self.certificate[..self.certificate_len as usize]
    }
    pub fn set_certificate(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.len() > 255 {
            return Err(Error::PayloadTooLarge);
        }
        self.certificate.fill(0);
        self.certificate[..bytes.len()].copy_from_slice(bytes);
        self.certificate_len = bytes.len() as u8;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeConfig {
    pub id: NodeId,
    pub name: NodeName,
    pub software: SoftwareVersion,
    pub hardware: HardwareVersion,
}
impl NodeConfig {
    pub fn info(&self, status: NodeStatus) -> NodeInfo {
        NodeInfo {
            status,
            name: self.name.clone(),
            software: self.software,
            hardware: self.hardware.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeInfo {
    pub status: NodeStatus,
    pub name: NodeName,
    pub software: SoftwareVersion,
    pub hardware: HardwareVersion,
}
impl NodeInfo {
    pub const SERVICE: ServiceType = GET_NODE_INFO;
    fn native(&self) -> dsdl::uavcan_protocol_GetNodeInfoResponse {
        let mut info: dsdl::uavcan_protocol_GetNodeInfoResponse = unsafe { core::mem::zeroed() };
        info.status = self.status.native();
        info.software_version.major = self.software.major;
        info.software_version.minor = self.software.minor;
        if let Some(commit) = self.software.vcs_commit {
            info.software_version.optional_field_flags |= 1;
            info.software_version.vcs_commit = commit;
        }
        if let Some(crc) = self.software.image_crc {
            info.software_version.optional_field_flags |= 2;
            info.software_version.image_crc = crc;
        }
        info.hardware_version.major = self.hardware.major;
        info.hardware_version.minor = self.hardware.minor;
        info.hardware_version.unique_id = self.hardware.unique_id;
        info.hardware_version.certificate_of_authenticity.data = self.hardware.certificate;
        info.hardware_version.certificate_of_authenticity.len = self.hardware.certificate_len;
        info.name.data = self.name.data;
        info.name.len = self.name.len; // bytes in the string, never backing-array capacity
        info
    }
    pub fn decode<const MAX: usize>(transfer: &ReceivedTransfer<MAX>) -> Result<Self, Error> {
        if transfer.metadata.kind != TransferKind::Response
            || transfer.metadata.data_type_id != GET_NODE_INFO.id as u16
        {
            return Err(Error::InvalidTransfer);
        }
        let native = transfer.native();
        let mut info: dsdl::uavcan_protocol_GetNodeInfoResponse = unsafe { core::mem::zeroed() };
        if unsafe { dsdl::_uavcan_protocol_GetNodeInfoResponse_decode(&native, &mut info) } {
            return Err(Error::Decode);
        }
        let name = core::str::from_utf8(
            info.name
                .data
                .get(..info.name.len as usize)
                .ok_or(Error::Decode)?,
        )
        .map_err(|_| Error::Decode)?;
        // A remote node may report an undefined UID. Preserve it on receive;
        // local configuration must instead pass HardwareVersion::new().
        let hardware = HardwareVersion {
            major: info.hardware_version.major,
            minor: info.hardware_version.minor,
            unique_id: info.hardware_version.unique_id,
            certificate: info.hardware_version.certificate_of_authenticity.data,
            certificate_len: info.hardware_version.certificate_of_authenticity.len,
        };
        Ok(Self {
            status: NodeStatus::from_native(info.status)?,
            name: NodeName::new(name)?,
            hardware,
            software: SoftwareVersion {
                major: info.software_version.major,
                minor: info.software_version.minor,
                vcs_commit: (info.software_version.optional_field_flags & 1 != 0)
                    .then_some(info.software_version.vcs_commit),
                image_crc: (info.software_version.optional_field_flags & 2 != 0)
                    .then_some(info.software_version.image_crc),
            },
        })
    }
}

impl<const N: usize, const MAX: usize, const SESSIONS: usize> Node<'_, N, MAX, SESSIONS> {
    pub fn broadcast_status(&mut self, status: NodeStatus, now_us: u64) -> Result<u16, Error> {
        let mut native = status.native();
        let mut buffer = [0; dsdl::UAVCAN_PROTOCOL_NODESTATUS_MAX_SIZE as usize];
        #[cfg(canard_tao)]
        let len = unsafe {
            dsdl::_uavcan_protocol_NodeStatus_encode(
                &mut native,
                buffer.as_mut_ptr(),
                !self.uses_can_fd(),
            )
        } as usize;
        #[cfg(not(canard_tao))]
        let len =
            unsafe { dsdl::_uavcan_protocol_NodeStatus_encode(&mut native, buffer.as_mut_ptr()) }
                as usize;
        self.broadcast(NODE_STATUS, &buffer[..len], Priority::LOW, now_us)
    }
    pub fn respond_node_info(
        &mut self,
        request: &ReceivedTransfer<MAX>,
        info: &NodeInfo,
        now_us: u64,
    ) -> Result<u16, Error> {
        if !request.payload().is_empty() {
            return Err(Error::InvalidTransfer);
        }
        let mut native = info.native();
        let mut buffer = [0; dsdl::UAVCAN_PROTOCOL_GETNODEINFO_RESPONSE_MAX_SIZE as usize];
        #[cfg(canard_tao)]
        let len = unsafe {
            dsdl::_uavcan_protocol_GetNodeInfoResponse_encode(
                &mut native,
                buffer.as_mut_ptr(),
                !self.uses_can_fd(),
            )
        } as usize;
        #[cfg(not(canard_tao))]
        let len = unsafe {
            dsdl::_uavcan_protocol_GetNodeInfoResponse_encode(&mut native, buffer.as_mut_ptr())
        } as usize;
        self.respond(GET_NODE_INFO, &request.metadata, &buffer[..len], now_us)
    }
}
