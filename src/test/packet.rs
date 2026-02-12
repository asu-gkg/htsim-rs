use crate::net::{Ecn, NodeId, Packet, TcpSegment, Transport};

#[test]
fn packet_preset_next_and_advance_walks_path() {
    let path = vec![NodeId(1), NodeId(2), NodeId(3)];
    let mut pkt = Packet::new_preset(1, 10, 100, path);
    assert_eq!(pkt.src, NodeId(1));
    assert_eq!(pkt.dst, NodeId(3));
    assert_eq!(pkt.hops_taken, 0);
    assert_eq!(pkt.preset_next(), Some(NodeId(2)));

    pkt = pkt.advance();
    assert_eq!(pkt.hops_taken, 1);
    assert_eq!(pkt.preset_next(), Some(NodeId(3)));

    pkt = pkt.advance();
    assert_eq!(pkt.hops_taken, 2);
    assert_eq!(pkt.preset_next(), None);
}

#[test]
fn packet_mixed_consumes_prefix_then_returns_none() {
    // Prefix length 2: one preset hop, then dynamic routing.
    let prefix = vec![NodeId(0), NodeId(1)];
    let mut pkt = Packet::new_mixed(1, 10, 100, prefix, NodeId(9));
    assert_eq!(pkt.src, NodeId(0));
    assert_eq!(pkt.dst, NodeId(9));
    assert_eq!(pkt.preset_next(), Some(NodeId(1)));

    pkt = pkt.advance();
    // Prefix exhausted (at last prefix node), so preset_next should return None.
    assert_eq!(pkt.preset_next(), None);

    pkt = pkt.advance();
    assert_eq!(pkt.preset_next(), None);
}

#[test]
fn packet_mixed_walks_full_prefix_in_order() {
    let prefix = vec![NodeId(0), NodeId(1), NodeId(2)];
    let mut pkt = Packet::new_mixed(1, 10, 100, prefix, NodeId(9));
    assert_eq!(pkt.preset_next(), Some(NodeId(1)));

    pkt = pkt.advance();
    assert_eq!(pkt.preset_next(), Some(NodeId(2)));

    pkt = pkt.advance();
    assert_eq!(pkt.preset_next(), None);
}

#[test]
fn packet_dynamic_has_no_preset_next() {
    let mut pkt = Packet::new_dynamic(1, 10, 100, NodeId(2), NodeId(7));
    assert_eq!(pkt.src, NodeId(2));
    assert_eq!(pkt.dst, NodeId(7));
    assert_eq!(pkt.preset_next(), None);

    pkt = pkt.advance();
    assert_eq!(pkt.hops_taken, 1);
    assert_eq!(pkt.preset_next(), None);
}

#[test]
fn packet_mark_ce_if_ect_only_marks_ect0() {
    let mut pkt = Packet::new_dynamic(1, 10, 100, NodeId(0), NodeId(1));

    pkt.ecn = Ecn::NotEct;
    pkt.mark_ce_if_ect();
    assert_eq!(pkt.ecn, Ecn::NotEct);

    pkt.ecn = Ecn::Ect0;
    pkt.mark_ce_if_ect();
    assert_eq!(pkt.ecn, Ecn::Ce);

    pkt.ecn = Ecn::Ce;
    pkt.mark_ce_if_ect();
    assert_eq!(pkt.ecn, Ecn::Ce);
}

#[test]
fn ecn_helpers_match_expected_states() {
    assert!(Ecn::Ect0.is_ect());
    assert!(!Ecn::NotEct.is_ect());
    assert!(!Ecn::Ce.is_ect());

    assert!(Ecn::Ce.is_ce());
    assert!(!Ecn::NotEct.is_ce());
    assert!(!Ecn::Ect0.is_ce());
}

#[test]
fn packet_transport_tag_defaults_to_none_and_is_mutable() {
    let mut pkt = Packet::new_dynamic(1, 10, 100, NodeId(0), NodeId(1));
    match pkt.transport {
        Transport::None => {}
        _ => panic!("expected Transport::None"),
    }

    pkt.transport = Transport::Tcp(TcpSegment::Ack { ack: 123 });
    match pkt.transport {
        Transport::Tcp(TcpSegment::Ack { ack }) => assert_eq!(ack, 123),
        _ => panic!("expected Transport::Tcp Ack"),
    }
}

#[test]
fn packet_advance_saturates_hops_taken() {
    let mut pkt = Packet::new_dynamic(1, 10, 100, NodeId(0), NodeId(1));
    pkt.hops_taken = u32::MAX;
    pkt = pkt.advance();
    assert_eq!(pkt.hops_taken, u32::MAX);
}

#[test]
#[should_panic]
fn packet_new_preset_panics_on_empty_path() {
    let _ = Packet::new_preset(1, 10, 100, Vec::new());
}

#[test]
#[should_panic]
fn packet_new_mixed_panics_on_empty_prefix() {
    let _ = Packet::new_mixed(1, 10, 100, Vec::new(), NodeId(9));
}
