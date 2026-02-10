//! 仿真器
//!
//! 定义事件驱动仿真器，维护当前时间与事件队列。

use super::event::Event;
use super::scheduled_event::ScheduledEvent;
use super::time::SimTime;
use super::world::World;
use std::collections::BinaryHeap;
use tracing::{debug, info, trace};

/// 事件驱动仿真器：维护当前时间与事件队列。
#[derive(Default)]
pub struct Simulator {
    now: SimTime,
    next_seq: u64,
    q: BinaryHeap<ScheduledEvent>,
}

impl Simulator {
    /// 获取当前仿真时间
    pub fn now(&self) -> SimTime {
        self.now
    }

    /// 调度事件在指定时间执行
    #[tracing::instrument(skip(self, ev), fields(event_type = std::any::type_name::<E>(), schedule_at = ?at))]
    pub fn schedule<E: Event>(&mut self, at: SimTime, ev: E) {
        let seq = self.next_seq;
        trace!(now = ?self.now, seq, "调度事件");

        self.next_seq = self.next_seq.wrapping_add(1);
        self.q.push(ScheduledEvent {
            at,
            seq,
            ev: Box::new(ev),
        });

        debug!(queue_size = self.q.len(), "事件已加入队列");
    }

    /// 运行直到事件队列为空或到达 `until`。
    pub fn run_until(&mut self, until: SimTime, world: &mut dyn World) {
        while let Some(top) = self.q.peek() {
            if top.at > until {
                break;
            }
            let item = self.q.pop().expect("peek then pop");
            self.now = item.at;
            item.ev.execute(self, world);
            world.on_tick(self);
        }
        self.now = self.now.max(until);
    }

    /// 运行所有事件直到队列为空。
    #[tracing::instrument(skip(self, world))]
    pub fn run(&mut self, world: &mut dyn World) {
        info!("▶️  开始运行仿真");
        debug!(now = ?self.now, queue_size = self.q.len(), "初始状态");

        let mut event_count = 0;
        while let Some(item) = self.q.pop() {
            event_count += 1;
            self.now = item.at;

            debug!(
                event_num = event_count,
                now = ?self.now,
                scheduled_at = ?item.at,
                seq = item.seq,
                remaining_queue = self.q.len(),
                "执行事件"
            );

            item.ev.execute(self, world);
            world.on_tick(self);
        }

        info!(
            total_events = event_count,
            final_time = ?self.now,
            "✅ 仿真完成"
        );
    }
}
