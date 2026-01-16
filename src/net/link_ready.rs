//! 链路就绪事件（用于驱动队列出队）

use super::id::LinkId;
use super::net_world::NetWorld;
use crate::sim::{Event, Simulator, World};

/// 事件：链路完成一次序列化发送后，在 depart 时刻触发，尝试发送队列中的下一个 packet。
#[derive(Debug)]
pub struct LinkReady {
    pub link_id: LinkId,
}

impl Event for LinkReady {
    fn execute(self: Box<Self>, sim: &mut Simulator, world: &mut dyn World) {
        let LinkReady { link_id } = *self;
        let w = world
            .as_any_mut()
            .downcast_mut::<NetWorld>()
            .expect("world must be NetWorld");
        w.net.on_link_ready(link_id, sim);
    }
}

