//! Transport-layer tags carried by packets.

/// Packet transport metadata.
///
/// `Packet` is a network-layer carrier; transport tags enable protocol simulation
/// without coupling the network to protocol implementations.
#[derive(Debug, Clone, Default)]
pub enum Transport {
    /// No transport metadata (default).
    #[default]
    None,
    /// TCP segment (simplified).
    Tcp(TcpSegment),
    /// DCTCP segment (simplified).
    Dctcp(DctcpSegment),
}

/// TCP segment (minimal fields for simulation).
#[derive(Debug, Clone)]
pub enum TcpSegment {
    /// SYN
    Syn,
    /// SYN-ACK
    SynAck,
    /// ACK for handshake
    HandshakeAck,
    /// Data segment: `seq` is byte sequence number, `len` is payload bytes.
    Data { seq: u64, len: u32 },
    /// ACK segment: `ack` is next expected byte (cumulative).
    Ack { ack: u64 },
}

/// DCTCP segment (minimal fields for simulation).
#[derive(Debug, Clone)]
pub enum DctcpSegment {
    /// Data segment: `seq` is byte sequence number, `len` is payload bytes.
    Data { seq: u64, len: u32 },
    /// ACK segment: `ack` is next expected byte (cumulative).
    Ack { ack: u64, ecn_echo: bool },
}
