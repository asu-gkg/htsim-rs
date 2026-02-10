//! Priority queue with drop-tail capacity.
//!
//! This queue gives strict priority to control traffic (e.g., TCP/DCTCP ACKs)
//! over bulk data packets. It helps avoid ACK starvation when bidirectional
//! data flows share the same egress queue.

use std::collections::VecDeque;

use crate::net::{DctcpSegment, Packet, TcpSegment, Transport};

use super::PacketQueue;

#[derive(Debug)]
pub struct PriorityQueue {
    max_bytes: u64,
    cur_bytes: u64,
    hi: VecDeque<Packet>,
    lo: VecDeque<Packet>,
}

impl PriorityQueue {
    pub fn new(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            cur_bytes: 0,
            hi: VecDeque::new(),
            lo: VecDeque::new(),
        }
    }

    fn is_high_priority(pkt: &Packet) -> bool {
        match &pkt.transport {
            Transport::Tcp(TcpSegment::Ack { .. })
            | Transport::Tcp(TcpSegment::Syn)
            | Transport::Tcp(TcpSegment::SynAck)
            | Transport::Tcp(TcpSegment::HandshakeAck)
            | Transport::Dctcp(DctcpSegment::Ack { .. }) => true,
            _ => false,
        }
    }
}

impl PacketQueue for PriorityQueue {
    fn enqueue(&mut self, pkt: Packet) -> Result<(), Packet> {
        let sz = pkt.size_bytes as u64;
        if self.cur_bytes.saturating_add(sz) > self.max_bytes {
            return Err(pkt);
        }
        self.cur_bytes = self.cur_bytes.saturating_add(sz);
        if Self::is_high_priority(&pkt) {
            self.hi.push_back(pkt);
        } else {
            self.lo.push_back(pkt);
        }
        Ok(())
    }

    fn dequeue(&mut self) -> Option<Packet> {
        let pkt = self.hi.pop_front().or_else(|| self.lo.pop_front())?;
        self.cur_bytes = self.cur_bytes.saturating_sub(pkt.size_bytes as u64);
        Some(pkt)
    }

    fn len(&self) -> usize {
        self.hi.len().saturating_add(self.lo.len())
    }

    fn bytes(&self) -> u64 {
        self.cur_bytes
    }

    fn capacity_bytes(&self) -> u64 {
        self.max_bytes
    }
}
