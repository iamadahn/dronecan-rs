use core::{
    convert::Infallible,
    future::{pending, poll_fn},
};
use dronecan_core::{
    Arena, Frame, MEM_BLOCK_SIZE, Node, NodeConfig, NodeInfo, NodeStatus, Subscription,
};
use embassy_futures::select::{Either3, select3};
use embassy_stm32::can;
use embassy_sync::channel::{Channel, TrySendError};
use embassy_time::{Duration, Instant, Ticker};
use embedded_can::Id;

const ARENA_BYTES: usize = MEM_BLOCK_SIZE * 256;
const TX_CAPACITY: usize = 8;
const RX_CAPACITY: usize = 32;
const SUBSCRIPTIONS: &[Subscription] = &[Subscription::request(NodeInfo::SERVICE)];
type Protocol<'a> = Node<'a, ARENA_BYTES>;

#[cfg(not(feature = "canfd"))]
type TxBuffer = can::TxBuf<TX_CAPACITY>;
#[cfg(feature = "canfd")]
type TxBuffer = can::TxFdBuf<TX_CAPACITY>;
#[cfg(not(feature = "canfd"))]
type RxBuffer = can::RxBuf<RX_CAPACITY>;
#[cfg(feature = "canfd")]
type RxBuffer = can::RxFdBuf<RX_CAPACITY>;
#[cfg(not(feature = "canfd"))]
type WireFrame = can::frame::Frame;
#[cfg(feature = "canfd")]
type WireFrame = can::frame::FdFrame;

enum SendError {
    Full,
    InvalidFrame,
}

fn wire_frame(frame: &Frame) -> Result<WireFrame, SendError> {
    #[cfg(not(feature = "canfd"))]
    {
        WireFrame::new_extended(frame.id(), frame.data()).map_err(|_| SendError::InvalidFrame)
    }
    #[cfg(feature = "canfd")]
    {
        let id = embedded_can::ExtendedId::new(frame.id()).ok_or(SendError::InvalidFrame)?;
        let header = if frame.is_fd() {
            can::frame::Header::new_fd(id.into(), frame.data().len() as u8, false, false)
        } else {
            can::frame::Header::new(id.into(), frame.data().len() as u8, false)
        };
        WireFrame::new(header, frame.data()).map_err(|_| SendError::InvalidFrame)
    }
}

pub async fn run(can: can::Can<'static>, config: NodeConfig) -> ! {
    // Driver IRQs own the rings; this task is the only owner of protocol state.
    let tx_buffer = cortex_m::singleton!(: TxBuffer = Channel::new()).expect("single CAN runner");
    let rx_buffer = cortex_m::singleton!(: RxBuffer = Channel::new()).expect("single CAN runner");
    #[cfg(not(feature = "canfd"))]
    let can = can.buffered(tx_buffer, rx_buffer);
    #[cfg(feature = "canfd")]
    let can = can.buffered_fd(tx_buffer, rx_buffer);
    let mut tx = can.writer();
    let rx = can.reader();
    let mut arena = Arena::new();
    let mut node: Protocol<'_> =
        Node::new(&mut arena, config.id, SUBSCRIPTIONS).expect("valid arena");
    node.set_can_fd(cfg!(feature = "canfd"))
        .expect("supported transport");
    // Fixed schedule with margin below DroneCAN's maximum 1000 ms period.
    let mut maintenance = Ticker::every(Duration::from_millis(500));
    let mut status = NodeStatus::operational(0);
    let mut bus_errors = 0u32;
    let mut ticks = 0u32;
    if let Err(error) = node.broadcast_status(status, Instant::now().as_micros()) {
        defmt::warn!(
            "Initial heartbeat failed: {:?}",
            defmt::Debug2Format(&error)
        );
    }
    loop {
        // try_transmit pops only AFTER try_write accepts the owned frame. When
        // full, RX and maintenance still run; no await holds a libcanard head.
        loop {
            let result = node.try_transmit(|frame| {
                tx.try_write(wire_frame(frame)?)
                    .map_err(|TrySendError::Full(_)| SendError::Full)
            });
            match result {
                Ok(true) => {}
                Ok(false) | Err(SendError::Full) => break,
                Err(SendError::InvalidFrame) => {
                    defmt::error!("Invalid frame from DroneCAN encoder");
                    let _: Result<bool, Infallible> = node.try_transmit(|_| Ok(()));
                    break;
                }
            }
        }
        let pending_tx = node.has_pending_tx();
        let ready = async {
            if pending_tx {
                poll_fn(|cx| tx.poll_ready_to_send(cx)).await;
            } else {
                pending::<()>().await;
            }
        };
        // Timer first ensures continuous RX traffic cannot starve maintenance.
        match select3(maintenance.next(), rx.receive(), ready).await {
            Either3::First(_) => {
                let now = Instant::now();
                status.uptime_sec = now.as_secs().min(u32::MAX as u64) as u32;
                if let Err(error) = node.cleanup(now.as_micros()) {
                    defmt::warn!("DroneCAN cleanup failed: {:?}", defmt::Debug2Format(&error));
                }
                if let Err(error) = node.broadcast_status(status, now.as_micros()) {
                    defmt::warn!("Heartbeat failed: {:?}", defmt::Debug2Format(&error));
                }
                ticks = ticks.wrapping_add(1);
                if ticks % 20 == 0 {
                    let pool = node.pool_statistics();
                    let counters = node.counters();
                    defmt::info!(
                        "DroneCAN pool {}/{}, peak {}; RX errors {}, CRC {}, OOM {}, TX errors {}, bus errors {}",
                        pool.current_usage_blocks,
                        pool.capacity_blocks,
                        pool.peak_usage_blocks,
                        counters.rx_errors,
                        counters.crc_errors,
                        counters.out_of_memory,
                        counters.tx_errors,
                        bus_errors
                    );
                }
            }
            Either3::Second(Ok(envelope)) => {
                if envelope.frame.header().rtr() {
                    continue;
                }
                let Id::Extended(id) = envelope.frame.id() else {
                    continue;
                };
                #[cfg(not(feature = "canfd"))]
                let frame = Frame::new(id.as_raw(), envelope.frame.data());
                #[cfg(feature = "canfd")]
                let frame = Frame::new_on_interface(
                    id.as_raw(),
                    envelope.frame.data(),
                    envelope.frame.header().fdcan(),
                    0,
                );
                let now = Instant::now().as_micros();
                status.uptime_sec = (now / 1_000_000).min(u32::MAX as u64) as u32;
                match frame.and_then(|frame| node.receive(&frame, now)) {
                    Ok(Some(request)) => {
                        if let Err(error) =
                            node.respond_node_info(&request, &config.info(status), now)
                        {
                            defmt::warn!(
                                "GetNodeInfo response failed: {:?}",
                                defmt::Debug2Format(&error)
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        defmt::warn!("DroneCAN RX failed: {:?}", defmt::Debug2Format(&error))
                    }
                }
            }
            Either3::Second(Err(error)) => {
                bus_errors = bus_errors.saturating_add(1);
                defmt::warn!("CAN bus error: {:?}", defmt::Debug2Format(&error));
            }
            Either3::Third(_) => {}
        }
    }
}
