# htsim-rs Architecture

This document describes a proposed architecture plan and module responsibilities. It is a guide for
future refactors and feature work, not a binding spec.

## Layered Architecture (Proposed)

ASCII diagram (top -> bottom):

    +-----------------------------+
    |         src/bin/*           |
    |  CLI entry points, scenarios|
    +-----------------------------+
                 |
                 v
    +-----------------------------+
    |          src/topo           |
    |  Topology builders, traffic |
    |  sources for experiments    |
    +-----------------------------+
                 |
                 v
    +-----------------------------+
    |           src/cc            |
    |  Collective communication   |
    |  (allreduce, broadcast, ...)|
    +-----------------------------+
                 |
                 v
    +-----------------------------+
    |          src/proto          |
    |  Transport protocols (TCP)  |
    |  DCTCP, timers, state       |
    +-----------------------------+
                 |
                 v
    +-----------------------------+
    |          src/net            |
    |  Network domain model:      |
    |  nodes, links, packets,     |
    |  forwarding, routing, stats |
    +-----------------------------+
          |             |
          v             v
    +-----------------------------+
    |         src/queue           |
    |  Queue disciplines          |
    +-----------------------------+

                 v
    +-----------------------------+
    |          src/sim            |
    |  Event-driven simulation    |
    |  clock, scheduler, world    |
    +-----------------------------+

    +-----------------------------+
    |          src/viz            |
    |  Observability / logging    |
    |  (observer-based hooks)     |
    +-----------------------------+

Notes:
- The dependency direction should be one-way from higher layers to lower layers.
- Observability (viz/stats) should be attached via hooks to avoid core-path coupling.

## Module Responsibilities

### src/sim (Simulation Core)
- Event scheduling, time management, and the simulation loop.
- World lifecycle and generic event execution.
- No knowledge of networking, protocols, or topologies.

### src/net (Network Domain Model)
- Domain entities: Node, Link, Packet, Routing, Stats.
- Forwarding logic, link serialization, and packet delivery.
- Provides a minimal API surface for protocol layers.

### src/proto (Protocol Layer)
- TCP/DCTCP state machines and timers.
- Produces/consumes packets through the net API.
- Avoids deep access to Network internals; uses explicit interfaces.

### src/queue (Queue Disciplines)
- Pluggable queue strategies (DropTail, RED, CoDel, etc.).
- Each queue implements a shared trait used by net/link.

### src/topo (Topology Builders)
- Constructs networks and initial traffic patterns.
- Leaves protocol selection/configuration to callers.

### src/cc (Collective Communication)
- Collective communication algorithms (allreduce, broadcast, etc.).
- Defines CC scheduling/flows on top of a topology and protocol stack.

### src/bin (CLI Entrypoints)
- Scenario wiring: parse args, build topo, configure protocol,
  run simulator, output results.
- Minimal business logic; delegate to modules above.

### src/viz (Observability)
- Structured event emission for replay/visualization.
- Should be integrated through observer hooks rather than direct
  calls from core logic.

## Desired Dependency Rules

1. sim is the foundation; it depends on nothing else in the crate.
2. net depends only on sim (and std).
3. proto depends on net (via traits) and sim for timers.
4. cc depends on net/sim and is transport-agnostic (RingTransport provided by scenarios).
5. topo depends on net/proto/queue to assemble scenarios.
6. bin depends on topo/cc + other layers for configuration.
7. viz depends on a small observer trait exported from net or sim.

## Extension Points (Planned)

- Queue: `PacketQueue` trait for drop policies and AQM variants.
- Routing: a `RoutingPolicy` trait to swap ECMP variants.
- Protocols: protocol stacks plugged via `Transport`/`NetApi`.
- Observability: `NetObserver` hooks for logging/viz.

## Current Mapping (High Level)

- `src/sim/*`: already matches the core layer.
- `src/net/*`: contains model + forwarding + stats + viz hooks.
- `src/proto/*`: TCP and DCTCP stacks.
- `src/queue/*`: DropTail queue.
- `src/topo/*`: dumbbell/fat-tree builders.
- `src/cc/*`: ring collective scheduling (transport adapters live in bins).
- `src/bin/*`: scenario entry points.

This document is intentionally compact; a future iteration can add
module interaction diagrams and API sketches once refactoring begins.

## Design Details (Current Code)

This section explains how modules call each other today, what each module does in practice,
and provides conceptual diagrams. It aims to make the codebase easier to navigate.

### Module Call Relationships (Current)

High-level call paths (simplified):

    bin/* -> topo/* -> net::Network (build topology)
    bin/* -> cc::ring::start_ring_allreduce (schedule collective)
    bin/* provides RingTransport using proto::tcp or proto::dctcp

Core runtime paths:

    sim::Simulator -> Event::execute -> net::Network::deliver/on_link_ready
    net::Network -> proto::{TcpStack,DctcpStack} on packet delivery
    proto::{TcpStack,DctcpStack} -> net::NetApi (make_packet/forward_from/viz_*)
    proto::{TcpStack,DctcpStack} -> sim::Simulator (schedule timers)

Key integration points:
- `net::NetApi` is the boundary used by protocol stacks.
- `net::Network::on_delivered` dispatches packets to TCP/DCTCP stacks based on `Packet.transport`.
- `cc::ring` is protocol-agnostic; the CLI provides a `RingTransport` that calls into TCP/DCTCP.

### Module Responsibilities (Expanded)

src/sim
- Event loop and time; owns the scheduler and calls `Event::execute`.
- Key files: `src/sim/simulator.rs`, `src/sim/event.rs`, `src/sim/time.rs`.

src/net
- Owns network domain objects and the runtime network state.
- Key files:
  - `src/net/network.rs` (topology, forwarding, stats, viz hooks, protocol dispatch)
  - `src/net/packet.rs`, `src/net/transport.rs` (packet + transport tags)
  - `src/net/api.rs` (NetApi boundary used by protocols)
  - `src/net/proto_bridge.rs` (World -> NetApi + protocol stack access)
  - `src/net/node.rs`, `src/net/link.rs`, `src/net/routing.rs`

src/proto
- Implements transport protocols as state machines.
- Key files: `src/proto/tcp.rs`, `src/proto/dctcp.rs`.
- Protocol stacks only call `net::NetApi` plus `sim::Simulator` for timers.

src/cc
- Collective-communication algorithms (currently ring allreduce).
- Key file: `src/cc/ring.rs`.
- Does not assume TCP/DCTCP; uses `RingTransport` trait.

src/topo
- Builds reusable topologies and returns host/switch identifiers.
- Key files: `src/topo/dumbbell.rs`, `src/topo/fat_tree.rs`.

src/queue
- Queue disciplines for link buffering.
- Key files: `src/queue/mod.rs`, `src/queue/drop_tail.rs`.

src/viz
- Event types and logger for visualization.
- Key files: `src/viz/types.rs`, `src/viz/mod.rs`.

src/bin
- Scenario wiring and configuration; defines transports for CC.
- Examples: `src/bin/dumbbell_tcp.rs`, `src/bin/fat_tree_allreduce_tcp.rs`.

### Class Diagram (Conceptual)

    +------------------+       +------------------+
    | sim::Simulator   |<>-----| sim::Event       |
    +------------------+       +------------------+
             |                          ^
             v                          |
    +------------------+                |
    | sim::World       |                |
    +------------------+                |
             |                          |
             v                          |
    +------------------+                |
    | net::NetWorld    |                |
    +------------------+                |
             |                          |
             v                          |
    +------------------+     uses    +-----------------+
    | net::Network     |<----------->| net::NetApi      |
    +------------------+              +-----------------+
      |    |    |    |
      |    |    |    +------------------------------+
      |    |    |                                   |
      |    |    +----> stats/viz                     |
      |    +--------> routing/links/nodes            |
      +-------------> tcp::TcpStack / dctcp::DctcpStack

    +------------------+     has     +------------------+
    | tcp::TcpStack    |-----------> | tcp::TcpConn     |
    +------------------+             +------------------+

    +------------------+     has     +------------------+
    | dctcp::DctcpStack|-----------> | dctcp::DctcpConn  |
    +------------------+             +------------------+

    +------------------+     contains    +------------------+
    | net::Packet      |---------------> | net::Transport   |
    +------------------+                +------------------+
                                            | TcpSegment
                                            | DctcpSegment

### Flow Diagram: Packet Forwarding

    (1) topo builds Network nodes/links
    (2) bin injects a packet (or proto sends a packet)
    (3) net::Network::forward_from enqueues on a link
    (4) sim schedules DeliverPacket + LinkReady
    (5) DeliverPacket -> node.on_packet -> forward_from / on_delivered
    (6) on_delivered -> dispatch to TCP/DCTCP based on Packet.transport

### Flow Diagram: TCP Data + ACK

    (1) bin creates TcpConn and schedules TcpStart
    (2) TcpStart inserts conn into TcpStack and calls send_data_if_possible
    (3) TcpStack builds Packet via NetApi and calls forward_from
    (4) on_delivered at dst -> TcpStack::on_tcp_segment(Data)
    (5) TcpStack sends ACK -> forward_from
    (6) on_delivered at src -> TcpStack::on_tcp_segment(Ack)
    (7) cwnd/rto updated; more data sent; RTO timers via Simulator

### Flow Diagram: Ring Allreduce (CC)

    (1) bin selects protocol (TCP/DCTCP) and builds a RingTransport adapter
    (2) bin calls cc::ring::start_ring_allreduce
    (3) cc schedules a step with N flows (rank i -> rank i+1)
    (4) each flow uses the RingTransport to start TCP/DCTCP connections
    (5) when all flows finish, cc schedules the next step
    (6) cc reports start/done timestamps and per-flow FCTs via handle.stats()
