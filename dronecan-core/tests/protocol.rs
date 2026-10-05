use dronecan_core::*;
use std::convert::Infallible;

const BYTES: usize = MEM_BLOCK_SIZE * 256;
const INFO_REQUEST: &[Subscription] = &[Subscription::request(NodeInfo::SERVICE)];
const INFO_RESPONSE: &[Subscription] = &[Subscription::response(NodeInfo::SERVICE)];
const STATUS: &[Subscription] = &[Subscription::message(NodeStatus::TYPE)];

fn id(value: u8) -> NodeId {
    NodeId::new(value).unwrap()
}
fn frames<const N: usize, const MAX: usize, const S: usize>(
    node: &mut Node<'_, N, MAX, S>,
) -> Vec<Frame> {
    let mut result = Vec::new();
    while node
        .try_transmit(|frame| {
            result.push(*frame);
            Ok::<_, Infallible>(())
        })
        .unwrap()
    {}
    result
}
fn feed<const N: usize, const MAX: usize, const S: usize>(
    node: &mut Node<'_, N, MAX, S>,
    input: &[Frame],
    time: u64,
) -> Result<Option<ReceivedTransfer<MAX>>, Error> {
    let mut result = None;
    for frame in input {
        if let Some(transfer) = node.receive(frame, time)? {
            result = Some(transfer);
        }
    }
    Ok(result)
}
fn config() -> NodeConfig {
    NodeConfig {
        id: id(69),
        name: NodeName::new("alan.battery").unwrap(),
        software: SoftwareVersion {
            major: 1,
            minor: 2,
            vcs_commit: Some(0x12345678),
            image_crc: Some(0x8877665544332211),
        },
        hardware: HardwareVersion::new(3, 4, [42; 16]).unwrap(),
    }
}
fn request_frame(source: u8, tid: u8) -> Frame {
    Frame::new(
        (1 << 16) | (1 << 15) | (69 << 8) | (1 << 7) | source as u32,
        &[0xc0 | tid],
    )
    .unwrap()
}

#[test]
fn get_node_info_round_trip_preserves_metadata_and_exact_name() {
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let mut client: Node<'_, BYTES> = Node::new(&mut a, id(42), INFO_RESPONSE).unwrap();
    let mut server: Node<'_, BYTES> = Node::new(&mut b, id(69), INFO_REQUEST).unwrap();
    client
        .request(NodeInfo::SERVICE, id(69), &[], Priority::HIGH, 100)
        .unwrap();
    let request = feed(&mut server, &frames(&mut client), 200)
        .unwrap()
        .unwrap();
    let expected = config().info(NodeStatus::operational(123));
    assert_eq!(
        server.respond_node_info(&request, &expected, 200).unwrap(),
        8
    );
    let reply = feed(&mut client, &frames(&mut server), 300)
        .unwrap()
        .unwrap();
    assert_eq!(reply.metadata.kind, TransferKind::Response);
    assert_eq!(reply.metadata.source_node_id, 69);
    assert_eq!(reply.metadata.transfer_id, request.metadata.transfer_id);
    assert_eq!(reply.metadata.priority, Priority::HIGH);
    assert_eq!(reply.payload().len(), 53);
    assert_eq!(NodeInfo::decode(&reply).unwrap(), expected);
    // Owned payload survives future calls, cleanup and freeing the native pool.
    client.cleanup(3_000_000).unwrap();
    assert_eq!(
        NodeInfo::decode(&reply).unwrap().name.as_str(),
        "alan.battery"
    );
}

#[test]
fn heartbeat_matches_known_wire_bytes() {
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let mut sender: Node<'_, BYTES> = Node::new(&mut a, id(69), &[]).unwrap();
    let mut receiver: Node<'_, BYTES> = Node::new(&mut b, id(42), STATUS).unwrap();
    sender
        .broadcast_status(NodeStatus::operational(0x01020304), 100)
        .unwrap();
    let output = frames(&mut sender);
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].id(), (24 << 24) | (341 << 8) | 69);
    assert_eq!(output[0].data(), &[4, 3, 2, 1, 0, 0, 0, 0xc0]);
    let transfer = feed(&mut receiver, &output, 200).unwrap().unwrap();
    assert_eq!(
        NodeStatus::decode(&transfer).unwrap(),
        NodeStatus::operational(0x01020304)
    );
}

#[test]
fn subscription_distinguishes_message_request_and_response_with_same_id() {
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let mut sender: Node<'_, BYTES> = Node::new(&mut a, id(42), &[]).unwrap();
    let mut server: Node<'_, BYTES> = Node::new(&mut b, id(69), INFO_REQUEST).unwrap();
    sender
        .broadcast(
            MessageType {
                id: 1,
                signature: 123,
            },
            &[0; 20],
            Priority::HIGH,
            100,
        )
        .unwrap();
    assert_eq!(feed(&mut server, &frames(&mut sender), 200).unwrap(), None);
    let metadata = TransferMetadata {
        data_type_id: 1,
        kind: TransferKind::Request,
        source_node_id: 69,
        transfer_id: 0,
        priority: Priority::HIGH,
        timestamp_us: 100,
    };
    sender
        .respond(NodeInfo::SERVICE, &metadata, &[0; 20], 300)
        .unwrap();
    assert_eq!(feed(&mut server, &frames(&mut sender), 400).unwrap(), None);
    assert_eq!(server.counters().rx_transfers, 0);
    assert!(server.counters().filtered > 0);
    assert_eq!(server.receive(&request_frame(0, 0), 450).unwrap(), None);
    assert!(
        server
            .receive(&request_frame(42, 1), 500)
            .unwrap()
            .is_some()
    );
}

#[test]
fn multi_frame_signature_is_checked_and_corruption_is_reported() {
    let ty = MessageType {
        id: 200,
        signature: 0xfedcba9876543210,
    };
    let subs = [Subscription::message(ty)];
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let mut sender: Node<'_, BYTES> = Node::new(&mut a, id(42), &[]).unwrap();
    let mut receiver: Node<'_, BYTES> = Node::new(&mut b, id(69), &subs).unwrap();
    let payload: Vec<_> = (0..150).collect();
    sender.broadcast(ty, &payload, Priority::HIGH, 100).unwrap();
    let output = frames(&mut sender);
    assert_eq!(
        feed(&mut receiver, &output, 200)
            .unwrap()
            .unwrap()
            .payload(),
        payload
    );
    sender.broadcast(ty, &payload, Priority::HIGH, 300).unwrap();
    let mut output = frames(&mut sender);
    let mut bytes = output[1].data().to_vec();
    bytes[0] ^= 1;
    output[1] = Frame::new(output[1].id(), &bytes).unwrap();
    assert_eq!(
        feed(&mut receiver, &output, 400),
        Err(Error::Transport(-17))
    );
    assert_eq!(receiver.counters().crc_errors, 1);
    assert_eq!(receiver.counters().rx_transfers, 1);
}

#[test]
fn backpressure_keeps_unsent_head_and_new_priority_does_not_drop_frames() {
    let mut arena = Arena::<BYTES>::new();
    let mut node: Node<'_, BYTES> = Node::new(&mut arena, id(69), &[]).unwrap();
    node.broadcast_status(NodeStatus::operational(1), 100)
        .unwrap();
    let pool_before = node.pool_statistics().current_usage_blocks;
    assert_eq!(node.try_transmit(|_| Err("full")), Err("full"));
    assert_eq!(node.pool_statistics().current_usage_blocks, pool_before);
    let request = TransferMetadata {
        data_type_id: 1,
        kind: TransferKind::Request,
        source_node_id: 42,
        transfer_id: 9,
        priority: Priority::HIGH,
        timestamp_us: 100,
    };
    node.respond(NodeInfo::SERVICE, &request, &[1, 2], 200)
        .unwrap();
    let output = frames(&mut node);
    assert_eq!(output.len(), 2);
    assert_eq!(
        output[0].id(),
        (8 << 24) | (1 << 16) | (42 << 8) | (1 << 7) | 69
    );
    assert_eq!(output[0].data().last().unwrap() & 31, 9);
    assert_eq!(output[1].id(), (24 << 24) | (341 << 8) | 69);
    assert_eq!(node.pool_statistics().current_usage_blocks, 0);
    assert!(!node.has_pending_tx());
}

#[test]
fn transfer_ids_are_per_descriptor_and_wrap_modulo_32() {
    let mut arena = Arena::<BYTES>::new();
    let mut node: Node<'_, BYTES> = Node::new(&mut arena, id(69), &[]).unwrap();
    for i in 0..35 {
        node.broadcast_status(NodeStatus::operational(i), 100 + i as u64)
            .unwrap();
        assert_eq!(
            frames(&mut node)[0].data().last().unwrap() & 31,
            i as u8 & 31
        );
    }
    node.broadcast(
        MessageType {
            id: 500,
            signature: 1,
        },
        &[1],
        Priority::LOW,
        200,
    )
    .unwrap();
    assert_eq!(frames(&mut node)[0].data().last().unwrap() & 31, 0);
    for (destination, expected) in [(42, 0), (43, 0), (42, 1)] {
        node.request(NodeInfo::SERVICE, id(destination), &[], Priority::HIGH, 300)
            .unwrap();
        assert_eq!(frames(&mut node)[0].data().last().unwrap() & 31, expected);
    }
}

#[test]
fn stale_rx_cleanup_recovers_exhausted_pool() {
    const SMALL: usize = MEM_BLOCK_SIZE * 2;
    let mut arena = Arena::<SMALL>::new();
    let mut server: Node<'_, SMALL> = Node::new(&mut arena, id(69), INFO_REQUEST).unwrap();
    for source in 1..=2 {
        assert!(
            server
                .receive(&request_frame(source, 0), 100)
                .unwrap()
                .is_some()
        );
    }
    assert_eq!(
        server.receive(&request_frame(3, 0), 200),
        Err(Error::OutOfMemory)
    );
    assert_eq!(server.pool_statistics().current_usage_blocks, 2);
    assert_eq!(
        server.broadcast_status(NodeStatus::operational(0), 300),
        Err(Error::OutOfMemory)
    );
    assert!(!server.has_pending_tx());
    server.cleanup(3_000_000).unwrap();
    assert_eq!(server.pool_statistics().current_usage_blocks, 0);
    assert!(
        server
            .receive(&request_frame(3, 0), 3_000_001)
            .unwrap()
            .is_some()
    );
    assert_eq!(server.counters().out_of_memory, 2);
}

#[test]
fn oversized_received_transfer_is_rejected_and_payload_blocks_are_released() {
    let ty = MessageType {
        id: 200,
        signature: 7,
    };
    let subs = [Subscription::message(ty)];
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let mut sender: Node<'_, BYTES> = Node::new(&mut a, id(42), &[]).unwrap();
    let mut receiver: Node<'_, BYTES, 16> = Node::new(&mut b, id(69), &subs).unwrap();
    sender
        .broadcast(ty, &[1; 100], Priority::HIGH, 100)
        .unwrap();
    assert_eq!(
        feed(&mut receiver, &frames(&mut sender), 200),
        Err(Error::PayloadTooLarge)
    );
    assert_eq!(receiver.pool_statistics().current_usage_blocks, 1);
}

#[test]
fn full_tx_pool_rolls_back_partial_multiframe_allocation() {
    const SMALL: usize = MEM_BLOCK_SIZE * 2;
    let mut arena = Arena::<SMALL>::new();
    let mut node: Node<'_, SMALL> = Node::new(&mut arena, id(69), &[]).unwrap();
    let ty = MessageType {
        id: 200,
        signature: 7,
    };
    assert_eq!(
        node.broadcast(ty, &[1; 100], Priority::HIGH, 100),
        Err(Error::OutOfMemory)
    );
    assert_eq!(node.pool_statistics().current_usage_blocks, 0);
    node.broadcast(ty, &[1], Priority::HIGH, 200).unwrap();
    assert_eq!(frames(&mut node)[0].data().last().unwrap() & 31, 0);
}

#[test]
fn largest_node_info_round_trips_certificate_and_name() {
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let mut client: Node<'_, BYTES> = Node::new(&mut a, id(42), INFO_RESPONSE).unwrap();
    let mut server: Node<'_, BYTES> = Node::new(&mut b, id(69), INFO_REQUEST).unwrap();
    if CAN_FD_SUPPORTED {
        server.set_can_fd(true).unwrap();
    }
    let mut cfg = config();
    cfg.name = NodeName::new(&"a".repeat(80)).unwrap();
    cfg.hardware.set_certificate(&[0xab; 255]).unwrap();
    let info = cfg.info(NodeStatus::operational(1));
    let request = server.receive(&request_frame(42, 0), 100).unwrap().unwrap();
    server.respond_node_info(&request, &info, 200).unwrap();
    let reply = feed(&mut client, &frames(&mut server), 300)
        .unwrap()
        .unwrap();
    assert_eq!(NodeInfo::decode(&reply).unwrap(), info);
}

#[cfg(feature = "canfd")]
#[test]
fn can_fd_padding_cannot_overflow_the_native_transfer_length() {
    let ty = MessageType {
        id: 200,
        signature: 7,
    };
    let subscriptions = [Subscription::message(ty)];
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let mut sender: Node<'_, BYTES, 1023> = Node::new(&mut a, id(42), &[]).unwrap();
    let mut receiver: Node<'_, BYTES, 1023> = Node::new(&mut b, id(69), &subscriptions).unwrap();
    sender.set_can_fd(true).unwrap();
    assert_eq!(
        sender.broadcast(ty, &[1; 1023], Priority::HIGH, 100),
        Err(Error::PayloadTooLarge)
    );
    sender
        .broadcast(ty, &[1; 1020], Priority::HIGH, 200)
        .unwrap();
    let transfer = feed(&mut receiver, &frames(&mut sender), 300)
        .unwrap()
        .unwrap();
    assert_eq!(&transfer.payload()[..1020], &[1; 1020]);
    assert!(transfer.payload()[1020..].iter().all(|&b| b == 0));
}

#[test]
fn invalid_configuration_and_frames_are_rejected() {
    for value in [0, 128, 255] {
        assert_eq!(NodeId::new(value), Err(Error::InvalidNodeId));
    }
    assert_eq!(Priority::new(32), Err(Error::InvalidPriority));
    for name in ["", "Alan.battery", "alan battery", "батарея"] {
        assert_eq!(NodeName::new(name), Err(Error::InvalidName));
    }
    assert_eq!(NodeName::new(&"a".repeat(81)), Err(Error::InvalidName));
    assert_eq!(
        HardwareVersion::new(0, 0, [0; 16]),
        Err(Error::InvalidUniqueId)
    );
    assert_eq!(Frame::new(1 << 29, &[1]), Err(Error::InvalidFrame));
    assert_eq!(Frame::new(1, &[0; 9]), Err(Error::InvalidFrame));
    assert_eq!(Frame::new(1, &[]), Err(Error::InvalidFrame));
    assert_eq!(Frame::new_fd(1, &[0; 10]), Err(Error::InvalidFrame));
    let mut arena = Arena::<0>::new();
    assert!(matches!(
        Node::<'_, 0>::new(&mut arena, id(1), &[]),
        Err(Error::InvalidArena)
    ));
    let mut arena = Arena::<BYTES>::new();
    let subscriptions = [
        Subscription::message(MessageType {
            id: 1,
            signature: 1,
        }),
        Subscription::message(MessageType {
            id: 1,
            signature: 2,
        }),
    ];
    assert!(matches!(
        Node::<'_, BYTES>::new(&mut arena, id(1), &subscriptions),
        Err(Error::ConflictingSubscription)
    ));
}

#[test]
fn bounded_session_table_reports_capacity_without_resetting_existing_ids() {
    let mut arena = Arena::<BYTES>::new();
    let mut node: Node<'_, BYTES, 512, 1> = Node::new(&mut arena, id(69), &[]).unwrap();
    node.broadcast_status(NodeStatus::operational(0), 100)
        .unwrap();
    frames(&mut node);
    assert_eq!(
        node.request(NodeInfo::SERVICE, id(42), &[], Priority::HIGH, 200),
        Err(Error::TooManyTxSessions)
    );
    node.broadcast_status(NodeStatus::operational(1), 300)
        .unwrap();
    assert_eq!(frames(&mut node)[0].data().last().unwrap() & 31, 1);
    assert_eq!(node.cleanup(299), Err(Error::ClockWentBackwards));
}

#[cfg(feature = "canfd")]
#[test]
fn can_fd_round_trip_uses_explicit_array_lengths_and_still_sends_heartbeat() {
    let (mut a, mut b) = (Arena::<BYTES>::new(), Arena::<BYTES>::new());
    let client_subs = [
        Subscription::response(NodeInfo::SERVICE),
        Subscription::message(NodeStatus::TYPE),
    ];
    let mut client: Node<'_, BYTES> = Node::new(&mut a, id(42), &client_subs).unwrap();
    let mut server: Node<'_, BYTES> = Node::new(&mut b, id(69), INFO_REQUEST).unwrap();
    server.set_can_fd(true).unwrap();
    client.set_can_fd(true).unwrap();
    client
        .request(NodeInfo::SERVICE, id(69), &[], Priority::HIGH, 100)
        .unwrap();
    let request = feed(&mut server, &frames(&mut client), 200)
        .unwrap()
        .unwrap();
    let info = config().info(NodeStatus::operational(3));
    server.respond_node_info(&request, &info, 200).unwrap();
    let output = frames(&mut server);
    assert!(output.iter().all(Frame::is_fd));
    assert_eq!(output.len(), 1);
    let reply = feed(&mut client, &output, 300).unwrap().unwrap();
    assert_eq!(NodeInfo::decode(&reply).unwrap(), info);
    server
        .broadcast_status(NodeStatus::operational(3), 400)
        .unwrap();
    let heartbeat = feed(&mut client, &frames(&mut server), 500)
        .unwrap()
        .unwrap();
    assert_eq!(NodeStatus::decode(&heartbeat).unwrap().uptime_sec, 3);
}

#[cfg(feature = "deadline")]
#[test]
fn cleanup_expires_queued_tx_frames() {
    let mut arena = Arena::<BYTES>::new();
    let mut node: Node<'_, BYTES> = Node::new(&mut arena, id(69), &[]).unwrap();
    node.broadcast_status(NodeStatus::operational(0), 100)
        .unwrap();
    node.cleanup(1_000_101).unwrap();
    assert!(!node.has_pending_tx());
    assert_eq!(node.pool_statistics().current_usage_blocks, 0);
}
