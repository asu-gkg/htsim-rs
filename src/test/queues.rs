use crate::net::{DctcpSegment, NodeId, Packet, TcpSegment, Transport};
use crate::queue::{DEFAULT_PKT_BYTES, DropTailQueue, PacketQueue, PriorityQueue, mem_from_pkt};

fn dyn_pkt(id: u64, size_bytes: u32) -> Packet {
    Packet::new_dynamic(id, 0, size_bytes, NodeId(0), NodeId(1))
}

#[test]
fn droptail_queue_enforces_capacity_and_preserves_order() {
    let mut q = DropTailQueue::new(100);
    assert_eq!(q.capacity_bytes(), 100);
    assert_eq!(q.len(), 0);
    assert_eq!(q.bytes(), 0);

    assert!(q.enqueue(dyn_pkt(1, 60)).is_ok());
    assert_eq!(q.len(), 1);
    assert_eq!(q.bytes(), 60);

    let dropped = q.enqueue(dyn_pkt(2, 50)).expect_err("should drop");
    assert_eq!(dropped.id, 2);
    assert_eq!(q.len(), 1);
    assert_eq!(q.bytes(), 60);

    assert_eq!(q.dequeue().expect("pkt").id, 1);
    assert_eq!(q.len(), 0);
    assert_eq!(q.bytes(), 0);
    assert!(q.dequeue().is_none());
}

#[test]
fn droptail_queue_zero_sized_packets_do_not_consume_capacity() {
    let mut q = DropTailQueue::new(10);
    assert!(q.enqueue(dyn_pkt(1, 0)).is_ok());
    assert!(q.enqueue(dyn_pkt(2, 0)).is_ok());
    assert_eq!(q.len(), 2);
    assert_eq!(q.bytes(), 0);
    assert_eq!(q.dequeue().expect("pkt").id, 1);
    assert_eq!(q.dequeue().expect("pkt").id, 2);
    assert!(q.dequeue().is_none());
}

#[test]
fn priority_queue_dequeues_high_priority_before_low_priority() {
    let mut q = PriorityQueue::new(1_000);

    let mut lo = dyn_pkt(1, 100);
    lo.transport = Transport::Tcp(TcpSegment::Data { seq: 0, len: 100 });

    let mut hi = dyn_pkt(2, 40);
    hi.transport = Transport::Tcp(TcpSegment::Ack { ack: 100 });

    assert!(q.enqueue(lo).is_ok());
    assert!(q.enqueue(hi).is_ok());

    assert_eq!(q.dequeue().expect("pkt").id, 2);
    assert_eq!(q.dequeue().expect("pkt").id, 1);
    assert!(q.dequeue().is_none());
}

#[test]
fn priority_queue_treats_handshake_and_dctcp_ack_as_high_priority() {
    let mut q = PriorityQueue::new(1_000);

    let mut syn = dyn_pkt(1, 60);
    syn.transport = Transport::Tcp(TcpSegment::Syn);

    let mut synack = dyn_pkt(2, 60);
    synack.transport = Transport::Tcp(TcpSegment::SynAck);

    let mut hsack = dyn_pkt(3, 60);
    hsack.transport = Transport::Tcp(TcpSegment::HandshakeAck);

    let mut dctcp_ack = dyn_pkt(4, 60);
    dctcp_ack.transport = Transport::Dctcp(DctcpSegment::Ack {
        ack: 1,
        ecn_echo: false,
    });

    let mut data = dyn_pkt(5, 60);
    data.transport = Transport::Dctcp(DctcpSegment::Data { seq: 0, len: 60 });

    assert!(q.enqueue(data).is_ok());
    assert!(q.enqueue(syn).is_ok());
    assert!(q.enqueue(synack).is_ok());
    assert!(q.enqueue(hsack).is_ok());
    assert!(q.enqueue(dctcp_ack).is_ok());

    // All control packets come out before data, preserving FIFO within the hi class.
    assert_eq!(q.dequeue().expect("pkt").id, 1);
    assert_eq!(q.dequeue().expect("pkt").id, 2);
    assert_eq!(q.dequeue().expect("pkt").id, 3);
    assert_eq!(q.dequeue().expect("pkt").id, 4);
    assert_eq!(q.dequeue().expect("pkt").id, 5);
    assert!(q.dequeue().is_none());
}

#[test]
fn priority_queue_enforces_capacity_drop_tail() {
    let mut q = PriorityQueue::new(100);

    let mut data = dyn_pkt(1, 90);
    data.transport = Transport::Tcp(TcpSegment::Data { seq: 0, len: 90 });
    assert!(q.enqueue(data).is_ok());
    assert_eq!(q.bytes(), 90);

    let mut ack = dyn_pkt(2, 20);
    ack.transport = Transport::Tcp(TcpSegment::Ack { ack: 1 });
    let dropped = q.enqueue(ack).expect_err("should drop");
    assert_eq!(dropped.id, 2);
    assert_eq!(q.bytes(), 90);
    assert_eq!(q.len(), 1);
}

#[test]
fn mem_from_pkt_multiplies_default_packet_bytes_and_saturates() {
    assert_eq!(mem_from_pkt(0), 0);
    assert_eq!(mem_from_pkt(2), DEFAULT_PKT_BYTES.saturating_mul(2));
    assert_eq!(mem_from_pkt(u64::MAX), u64::MAX);
}

#[test]
fn priority_queue_len_and_bytes_track_enqueues_and_dequeues() {
    let mut q = PriorityQueue::new(1_000);

    let mut hi = dyn_pkt(1, 40);
    hi.transport = Transport::Tcp(TcpSegment::Ack { ack: 1 });
    let mut lo = dyn_pkt(2, 100);
    lo.transport = Transport::Tcp(TcpSegment::Data { seq: 0, len: 100 });

    assert!(q.enqueue(lo).is_ok());
    assert!(q.enqueue(hi).is_ok());
    assert_eq!(q.len(), 2);
    assert_eq!(q.bytes(), 140);

    assert_eq!(q.dequeue().expect("pkt").id, 1);
    assert_eq!(q.len(), 1);
    assert_eq!(q.bytes(), 100);

    assert_eq!(q.dequeue().expect("pkt").id, 2);
    assert_eq!(q.len(), 0);
    assert_eq!(q.bytes(), 0);
    assert!(q.dequeue().is_none());
}
