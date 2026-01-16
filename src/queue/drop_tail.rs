//! DropTail（尾丢弃）队列
//!
//! 当队列容量不足时，直接丢弃新到达的 packet。

use std::collections::VecDeque;

use crate::net::Packet;

use super::PacketQueue;

#[derive(Debug)]
pub struct DropTailQueue {
    max_bytes: u64,
    cur_bytes: u64,
    q: VecDeque<Packet>,
}

impl DropTailQueue {
    pub fn new(max_bytes: u64) -> Self {
        Self {
            max_bytes,
            cur_bytes: 0,
            q: VecDeque::new(),
        }
    }
}

impl PacketQueue for DropTailQueue {
    fn enqueue(&mut self, pkt: Packet) -> Result<(), Packet> {
        let sz = pkt.size_bytes as u64;
        if self.cur_bytes.saturating_add(sz) > self.max_bytes {
            return Err(pkt);
        }
        self.cur_bytes = self.cur_bytes.saturating_add(sz);
        self.q.push_back(pkt);
        Ok(())
    }

    fn dequeue(&mut self) -> Option<Packet> {
        let pkt = self.q.pop_front()?;
        self.cur_bytes = self.cur_bytes.saturating_sub(pkt.size_bytes as u64);
        Some(pkt)
    }

    fn len(&self) -> usize {
        self.q.len()
    }

    fn bytes(&self) -> u64 {
        self.cur_bytes
    }

    fn capacity_bytes(&self) -> u64 {
        self.max_bytes
    }
}
