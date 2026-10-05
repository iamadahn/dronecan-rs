use crate::{
    Error, Frame, MEM_BLOCK_SIZE, MessageType, NodeId, Priority, ServiceType, Subscription,
    TransferKind,
};
use canard_sys as ffi;
use core::{ffi::c_void, marker::PhantomData, mem::MaybeUninit, ptr};

/// Borrowed by a Node for its entire lifetime, so C's pool pointers remain valid
/// even if the Node itself moves. No self-referential Rust object is required.
#[repr(align(8))]
pub struct Arena<const N: usize>([u8; N]);
impl<const N: usize> Arena<N> {
    pub const fn new() -> Self {
        Self([0; N])
    }
}
impl<const N: usize> Default for Arena<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub rx_frames: u32,
    pub rx_transfers: u32,
    pub filtered: u32,
    pub rx_errors: u32,
    pub crc_errors: u32,
    pub out_of_memory: u32,
    pub tx_frames: u32,
    pub tx_errors: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolStatistics {
    pub capacity_blocks: u16,
    pub current_usage_blocks: u16,
    pub peak_usage_blocks: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferMetadata {
    pub data_type_id: u16,
    pub kind: TransferKind,
    pub source_node_id: u8,
    pub transfer_id: u8,
    pub priority: Priority,
    pub timestamp_us: u64,
}

/// Contiguous, owned transfer bytes; no pointer into libcanard survives RX.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReceivedTransfer<const MAX: usize = 512> {
    pub metadata: TransferMetadata,
    payload: [u8; MAX],
    len: u16,
    tao: bool,
}
impl<const MAX: usize> ReceivedTransfer<MAX> {
    pub fn payload(&self) -> &[u8] {
        &self.payload[..self.len as usize]
    }
    pub(crate) fn native(&self) -> ffi::CanardRxTransfer {
        // Null middle/tail select libcanard's contiguous-payload decoder path.
        let mut transfer: ffi::CanardRxTransfer = unsafe { core::mem::zeroed() };
        transfer.payload_head = self.payload.as_ptr();
        transfer.payload_len = self.len;
        transfer.data_type_id = self.metadata.data_type_id;
        transfer.transfer_type = self.metadata.kind.native() as u8;
        #[cfg(canard_tao)]
        {
            transfer.tao = self.tao;
        }
        transfer
    }
}

struct RxContext<'a, const MAX: usize> {
    subscriptions: &'a [Subscription],
    received: Option<Result<ReceivedTransfer<MAX>, Error>>,
}

unsafe extern "C" fn should_accept<const MAX: usize>(
    ins: *const ffi::CanardInstance,
    signature: *mut u64,
    id: u16,
    kind: ffi::CanardTransferType,
    source: u8,
) -> bool {
    if kind != ffi::CanardTransferType_CanardTransferTypeBroadcast && source == 0 {
        return false;
    }
    // Context lives on receive()'s stack for exactly this synchronous C call.
    let context = unsafe { &*((*ins).user_reference.cast::<RxContext<'_, MAX>>()) };
    if let Some(sub) = context
        .subscriptions
        .iter()
        .find(|s| s.id == id && s.kind.native() == kind)
    {
        unsafe {
            *signature = sub.signature;
        }
        true
    } else {
        false
    }
}

unsafe extern "C" fn on_reception<const MAX: usize>(
    ins: *mut ffi::CanardInstance,
    transfer: *mut ffi::CanardRxTransfer,
) {
    let context = unsafe { &mut *((*ins).user_reference.cast::<RxContext<'_, MAX>>()) };
    let transfer = unsafe { &*transfer };
    let result = (|| {
        if transfer.payload_len as usize > MAX {
            return Err(Error::PayloadTooLarge);
        }
        let tao = {
            #[cfg(canard_tao)]
            {
                transfer.tao
            }
            #[cfg(not(canard_tao))]
            {
                true
            }
        };
        let mut received = ReceivedTransfer {
            metadata: TransferMetadata {
                data_type_id: transfer.data_type_id,
                kind: TransferKind::from_native(transfer.transfer_type)?,
                source_node_id: transfer.source_node_id,
                transfer_id: transfer.transfer_id,
                priority: Priority::new(transfer.priority)?,
                timestamp_us: transfer.timestamp_usec,
            },
            payload: [0; MAX],
            len: transfer.payload_len,
            tao,
        };
        for (i, byte) in received.payload[..received.len as usize]
            .iter_mut()
            .enumerate()
        {
            // C checks payload bounds; the output points to a live u8.
            if unsafe {
                ffi::canardDecodeScalar(
                    transfer,
                    (i * 8) as u32,
                    8,
                    false,
                    (byte as *mut u8).cast(),
                )
            } != 8
            {
                return Err(Error::Decode);
            }
        }
        Ok(received)
    })();
    context.received = Some(result);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Descriptor {
    Broadcast(u16),
    Request(u8, NodeId),
}
#[derive(Clone, Copy)]
struct Session {
    descriptor: Descriptor,
    tid: u8,
}

pub struct Node<'a, const N: usize, const MAX: usize = 512, const SESSIONS: usize = 16> {
    instance: ffi::CanardInstance,
    subscriptions: &'a [Subscription],
    sessions: [Option<Session>; SESSIONS],
    counters: Counters,
    can_fd: bool,
    last_time_us: u64,
    _arena: PhantomData<&'a mut Arena<N>>,
}

impl<'a, const N: usize, const MAX: usize, const SESSIONS: usize> Node<'a, N, MAX, SESSIONS> {
    pub fn new(
        arena: &'a mut Arena<N>,
        id: NodeId,
        subscriptions: &'a [Subscription],
    ) -> Result<Self, Error> {
        if N < MEM_BLOCK_SIZE || N / MEM_BLOCK_SIZE > u16::MAX as usize || MAX > 1023 {
            return Err(Error::InvalidArena);
        }
        for (i, subscription) in subscriptions.iter().enumerate() {
            if subscriptions[..i].iter().any(|other| {
                other.id == subscription.id
                    && other.kind == subscription.kind
                    && other.signature != subscription.signature
            }) {
                return Err(Error::ConflictingSubscription);
            }
        }
        let mut instance = MaybeUninit::uninit();
        // The arena is aligned, exclusively borrowed, and outlives this Node.
        unsafe {
            ffi::canardInit(
                instance.as_mut_ptr(),
                arena.0.as_mut_ptr().cast(),
                N,
                Some(on_reception::<MAX>),
                Some(should_accept::<MAX>),
                ptr::null_mut(),
            );
            ffi::canardSetLocalNodeID(instance.as_mut_ptr(), id.get());
        }
        Ok(Self {
            instance: unsafe { instance.assume_init() },
            subscriptions,
            sessions: [None; SESSIONS],
            counters: Counters::default(),
            can_fd: false,
            last_time_us: 0,
            _arena: PhantomData,
        })
    }
    /// Sets TX framing. RX may accept classic and FD frames when compiled for FD.
    pub fn set_can_fd(&mut self, enabled: bool) -> Result<(), Error> {
        if enabled && !crate::CAN_FD_SUPPORTED {
            return Err(Error::UnsupportedCanFd);
        }
        self.can_fd = enabled;
        Ok(())
    }
    pub const fn uses_can_fd(&self) -> bool {
        self.can_fd
    }
    pub const fn counters(&self) -> Counters {
        self.counters
    }
    pub fn pool_statistics(&mut self) -> PoolStatistics {
        let s = unsafe { ffi::canardGetPoolAllocatorStatistics(&mut self.instance) };
        PoolStatistics {
            capacity_blocks: s.capacity_blocks,
            current_usage_blocks: s.current_usage_blocks,
            peak_usage_blocks: s.peak_usage_blocks,
        }
    }
    fn time(&mut self, now_us: u64) -> Result<(), Error> {
        if now_us < self.last_time_us {
            return Err(Error::ClockWentBackwards);
        }
        self.last_time_us = now_us;
        Ok(())
    }
    pub fn cleanup(&mut self, now_us: u64) -> Result<(), Error> {
        self.time(now_us)?;
        unsafe {
            ffi::canardCleanupStaleTransfers(&mut self.instance, now_us);
        }
        Ok(())
    }
    pub fn receive(
        &mut self,
        frame: &Frame,
        now_us: u64,
    ) -> Result<Option<ReceivedTransfer<MAX>>, Error> {
        self.time(now_us)?;
        self.counters.rx_frames = self.counters.rx_frames.saturating_add(1);
        let mut context = RxContext {
            subscriptions: self.subscriptions,
            received: None,
        };
        let native = frame.native();
        self.instance.user_reference = (&mut context as *mut RxContext<'_, MAX>).cast::<c_void>();
        // Callbacks are internal, synchronous and never call user code or unwind.
        let result = unsafe { ffi::canardHandleRxFrame(&mut self.instance, &native, now_us) };
        self.instance.user_reference = ptr::null_mut();
        if result == -(ffi::CANARD_ERROR_RX_NOT_WANTED as i16)
            || result == -(ffi::CANARD_ERROR_RX_WRONG_ADDRESS as i16)
        {
            self.counters.filtered = self.counters.filtered.saturating_add(1);
            return Ok(None);
        }
        if result < 0 {
            self.counters.rx_errors = self.counters.rx_errors.saturating_add(1);
            if result == -(ffi::CANARD_ERROR_RX_BAD_CRC as i16) {
                self.counters.crc_errors = self.counters.crc_errors.saturating_add(1);
            }
            if result == -(ffi::CANARD_ERROR_OUT_OF_MEMORY as i16) {
                self.counters.out_of_memory = self.counters.out_of_memory.saturating_add(1);
            }
            return Err(Error::transport(result));
        }
        match context.received {
            Some(Ok(transfer)) => {
                self.counters.rx_transfers = self.counters.rx_transfers.saturating_add(1);
                Ok(Some(transfer))
            }
            Some(Err(error)) => {
                self.counters.rx_errors = self.counters.rx_errors.saturating_add(1);
                Err(error)
            }
            None => Ok(None),
        }
    }
    /// Atomically hands the current head to a synchronous, nonblocking sink.
    /// Sink failure leaves the head queued. No raw queue pointer escapes.
    pub fn try_transmit<E>(
        &mut self,
        sink: impl FnOnce(&Frame) -> Result<(), E>,
    ) -> Result<bool, E> {
        let head = unsafe { ffi::canardPeekTxQueue(&self.instance) };
        if head.is_null() {
            return Ok(false);
        }
        let frame = Frame::from_native(unsafe { &*head });
        sink(&frame)?;
        unsafe {
            ffi::canardPopTxQueue(&mut self.instance);
        }
        self.counters.tx_frames = self.counters.tx_frames.saturating_add(1);
        Ok(true)
    }
    pub fn has_pending_tx(&self) -> bool {
        !unsafe { ffi::canardPeekTxQueue(&self.instance) }.is_null()
    }
    fn session(&mut self, descriptor: Descriptor) -> Result<usize, Error> {
        if let Some(i) = self
            .sessions
            .iter()
            .position(|s| s.is_some_and(|s| s.descriptor == descriptor))
        {
            return Ok(i);
        }
        let i = self
            .sessions
            .iter()
            .position(Option::is_none)
            .ok_or(Error::TooManyTxSessions)?;
        self.sessions[i] = Some(Session { descriptor, tid: 0 });
        Ok(i)
    }
    pub fn broadcast(
        &mut self,
        ty: MessageType,
        payload: &[u8],
        priority: Priority,
        now_us: u64,
    ) -> Result<u16, Error> {
        self.time(now_us)?;
        let i = self.session(Descriptor::Broadcast(ty.id))?;
        let mut tid = self.sessions[i].expect("session exists").tid;
        let result = self.enqueue(
            ty.id,
            ty.signature,
            TransferKind::Broadcast,
            None,
            &mut tid,
            payload,
            priority,
            now_us,
        );
        self.sessions[i].as_mut().expect("session exists").tid = tid;
        result
    }
    pub fn request(
        &mut self,
        ty: ServiceType,
        destination: NodeId,
        payload: &[u8],
        priority: Priority,
        now_us: u64,
    ) -> Result<u16, Error> {
        self.time(now_us)?;
        let i = self.session(Descriptor::Request(ty.id, destination))?;
        let mut tid = self.sessions[i].expect("session exists").tid;
        let result = self.enqueue(
            ty.id as u16,
            ty.signature,
            TransferKind::Request,
            Some(destination),
            &mut tid,
            payload,
            priority,
            now_us,
        );
        self.sessions[i].as_mut().expect("session exists").tid = tid;
        result
    }
    pub fn respond(
        &mut self,
        ty: ServiceType,
        request: &TransferMetadata,
        payload: &[u8],
        now_us: u64,
    ) -> Result<u16, Error> {
        self.time(now_us)?;
        if request.kind != TransferKind::Request
            || request.data_type_id != ty.id as u16
            || request.transfer_id > 31
        {
            return Err(Error::InvalidTransfer);
        }
        let destination = NodeId::new(request.source_node_id)?;
        let mut tid = request.transfer_id;
        self.enqueue(
            ty.id as u16,
            ty.signature,
            TransferKind::Response,
            Some(destination),
            &mut tid,
            payload,
            request.priority,
            now_us,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &mut self,
        id: u16,
        signature: u64,
        kind: TransferKind,
        destination: Option<NodeId>,
        tid: &mut u8,
        payload: &[u8],
        priority: Priority,
        now_us: u64,
    ) -> Result<u16, Error> {
        // libcanard's RX length field is 10 bits. FD DLC rounding adds zero
        // padding to the transfer; include it when checking the wire limit.
        let padding = if self.can_fd {
            let last_len = if payload.len() < 64 {
                payload.len() + 1
            } else {
                (payload.len() + 1) % 63 + 2
            };
            let dlc_len = [1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 20, 24, 32, 48, 64]
                .into_iter()
                .find(|&n| n >= last_len)
                .expect("valid FD length");
            dlc_len - last_len
        } else {
            0
        };
        if payload.len() > 1023 || payload.len().saturating_add(padding) > 1023 {
            return Err(Error::PayloadTooLarge);
        }
        let mut tx: ffi::CanardTxTransfer = unsafe { core::mem::zeroed() };
        tx.transfer_type = kind.native();
        tx.data_type_id = id;
        tx.data_type_signature = signature;
        tx.inout_transfer_id = tid;
        tx.priority = priority.get();
        tx.payload = payload.as_ptr();
        tx.payload_len = payload.len() as u16;
        #[cfg(canard_canfd)]
        {
            tx.canfd = self.can_fd;
        }
        #[cfg(canard_tao)]
        {
            tx.tao = !self.can_fd;
        }
        #[cfg(canard_deadline)]
        {
            tx.deadline_usec = now_us.saturating_add(1_000_000);
        }
        #[cfg(not(canard_deadline))]
        {
            let _ = now_us;
        }
        // This wrapper operates one physical interface. The native multi-iface
        // ABI can be enabled without pretending to implement redundant buses.
        #[cfg(canard_multi_iface)]
        {
            tx.iface_mask = 1;
        }
        let result = unsafe {
            match destination {
                Some(dest) => {
                    ffi::canardRequestOrRespondObj(&mut self.instance, dest.get(), &mut tx)
                }
                None => ffi::canardBroadcastObj(&mut self.instance, &mut tx),
            }
        };
        if result < 0 {
            self.counters.tx_errors = self.counters.tx_errors.saturating_add(1);
            if result == -(ffi::CANARD_ERROR_OUT_OF_MEMORY as i16) {
                self.counters.out_of_memory = self.counters.out_of_memory.saturating_add(1);
            }
            Err(Error::transport(result))
        } else {
            Ok(result as u16)
        }
    }
}
