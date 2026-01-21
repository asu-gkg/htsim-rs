//! Helpers for accessing protocol stacks from the simulation world.

use crate::proto::dctcp::DctcpStack;
use crate::proto::tcp::TcpStack;
use crate::sim::World;

use super::{NetApi, NetWorld};

pub(crate) fn with_tcp_stack<F, R>(world: &mut dyn World, f: F) -> R
where
    F: FnOnce(&mut dyn NetApi, &mut TcpStack) -> R,
{
    let w = world
        .as_any_mut()
        .downcast_mut::<NetWorld>()
        .expect("world must be NetWorld");
    let mut tcp = std::mem::take(&mut w.net.tcp);
    let result = f(&mut w.net, &mut tcp);
    w.net.tcp = tcp;
    result
}

pub(crate) fn with_dctcp_stack<F, R>(world: &mut dyn World, f: F) -> R
where
    F: FnOnce(&mut dyn NetApi, &mut DctcpStack) -> R,
{
    let w = world
        .as_any_mut()
        .downcast_mut::<NetWorld>()
        .expect("world must be NetWorld");
    let mut dctcp = std::mem::take(&mut w.net.dctcp);
    let result = f(&mut w.net, &mut dctcp);
    w.net.dctcp = dctcp;
    result
}
