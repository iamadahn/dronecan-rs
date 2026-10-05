use canard_sys as ffi;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidNodeId,
    InvalidPriority,
    InvalidName,
    InvalidUniqueId,
    InvalidArena,
    InvalidFrame,
    InvalidTransfer,
    ConflictingSubscription,
    PayloadTooLarge,
    Decode,
    OutOfMemory,
    TooManyTxSessions,
    UnsupportedCanFd,
    ClockWentBackwards,
    Transport(i16),
}

impl Error {
    pub(crate) fn transport(code: i16) -> Self {
        if code == -(ffi::CANARD_ERROR_OUT_OF_MEMORY as i16) {
            Self::OutOfMemory
        } else {
            Self::Transport(code)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(u8);
impl NodeId {
    pub const fn new(value: u8) -> Result<Self, Error> {
        if value >= 1 && value <= 127 {
            Ok(Self(value))
        } else {
            Err(Error::InvalidNodeId)
        }
    }
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Priority(u8);
impl Priority {
    pub const LOW: Self = Self(24);
    pub const HIGH: Self = Self(8);
    pub const fn new(value: u8) -> Result<Self, Error> {
        if value <= 31 {
            Ok(Self(value))
        } else {
            Err(Error::InvalidPriority)
        }
    }
    pub const fn get(self) -> u8 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferKind {
    Broadcast,
    Request,
    Response,
}
impl TransferKind {
    pub(crate) const fn native(self) -> ffi::CanardTransferType {
        match self {
            Self::Broadcast => ffi::CanardTransferType_CanardTransferTypeBroadcast,
            Self::Request => ffi::CanardTransferType_CanardTransferTypeRequest,
            Self::Response => ffi::CanardTransferType_CanardTransferTypeResponse,
        }
    }
    pub(crate) fn from_native(value: u8) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Response),
            1 => Ok(Self::Request),
            2 => Ok(Self::Broadcast),
            _ => Err(Error::InvalidTransfer),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageType {
    pub id: u16,
    pub signature: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceType {
    pub id: u8,
    pub signature: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub(crate) id: u16,
    pub(crate) signature: u64,
    pub(crate) kind: TransferKind,
}
impl Subscription {
    pub const fn message(ty: MessageType) -> Self {
        Self {
            id: ty.id,
            signature: ty.signature,
            kind: TransferKind::Broadcast,
        }
    }
    pub const fn request(ty: ServiceType) -> Self {
        Self {
            id: ty.id as u16,
            signature: ty.signature,
            kind: TransferKind::Request,
        }
    }
    pub const fn response(ty: ServiceType) -> Self {
        Self {
            id: ty.id as u16,
            signature: ty.signature,
            kind: TransferKind::Response,
        }
    }
}

/// An extended data frame, with no RTR/error flags. Private length and payload
/// prevent callers from constructing frames that could overrun native buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    id: u32,
    data: [u8; 64],
    len: u8,
    can_fd: bool,
    iface: u8,
}
impl Frame {
    pub fn new(id: u32, data: &[u8]) -> Result<Self, Error> {
        Self::new_on_interface(id, data, false, 0)
    }
    pub fn new_fd(id: u32, data: &[u8]) -> Result<Self, Error> {
        Self::new_on_interface(id, data, true, 0)
    }
    pub fn new_on_interface(id: u32, data: &[u8], can_fd: bool, iface: u8) -> Result<Self, Error> {
        let valid_len = if can_fd {
            matches!(data.len(), 0..=8 | 12 | 16 | 20 | 24 | 32 | 48 | 64)
        } else {
            data.len() <= 8
        };
        if id > ffi::CANARD_CAN_EXT_ID_MASK || data.is_empty() || !valid_len || iface > 7 {
            return Err(Error::InvalidFrame);
        }
        if can_fd && !crate::CAN_FD_SUPPORTED {
            return Err(Error::UnsupportedCanFd);
        }
        let mut frame = Self {
            id,
            data: [0; 64],
            len: data.len() as u8,
            can_fd,
            iface,
        };
        frame.data[..data.len()].copy_from_slice(data);
        Ok(frame)
    }
    pub const fn id(&self) -> u32 {
        self.id
    }
    pub fn data(&self) -> &[u8] {
        &self.data[..self.len as usize]
    }
    pub const fn is_fd(&self) -> bool {
        self.can_fd
    }
    pub const fn interface(&self) -> u8 {
        self.iface
    }
    pub(crate) fn native(&self) -> ffi::CanardCANFrame {
        // All-zero native fields are valid, including optional bools/pointers.
        let mut frame: ffi::CanardCANFrame = unsafe { core::mem::zeroed() };
        frame.id = self.id | ffi::CANARD_CAN_FRAME_EFF;
        frame.data_len = self.len;
        frame.data[..self.len as usize].copy_from_slice(self.data());
        frame.iface_id = self.iface;
        #[cfg(canard_canfd)]
        {
            frame.canfd = self.can_fd;
        }
        frame
    }
    pub(crate) fn from_native(frame: &ffi::CanardCANFrame) -> Self {
        // libcanard only enqueues extended data frames with a valid DLC.
        let can_fd = {
            #[cfg(canard_canfd)]
            {
                frame.canfd
            }
            #[cfg(not(canard_canfd))]
            {
                false
            }
        };
        Self::new_on_interface(
            frame.id & ffi::CANARD_CAN_EXT_ID_MASK,
            &frame.data[..frame.data_len as usize],
            can_fd,
            frame.iface_id,
        )
        .expect("libcanard generated an invalid frame")
    }
}
