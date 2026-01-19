//! 传输层/协议模块
//!
//! 包含 TCP / DCTCP 的简化实现（用于仿真实验）。

pub mod dctcp;
pub mod tcp;

/// 数据包携带的传输层信息。
///
/// `Packet` 本身是网络层的载体；为了支持协议仿真，我们允许其携带少量“传输层标签”。
#[derive(Debug, Clone, Default)]
pub enum Transport {
    /// 无传输层（默认，保持兼容：现有注入逻辑仍然只是“裸包”）
    #[default]
    None,
    /// TCP 段（简化）
    Tcp(TcpSegment),
    /// DCTCP 段（简化）
    Dctcp(DctcpSegment),
}

/// TCP 段（极简：只保留实验需要的字段）
#[derive(Debug, Clone)]
pub enum TcpSegment {
    /// SYN
    Syn,
    /// SYN-ACK
    SynAck,
    /// ACK for handshake
    HandshakeAck,
    /// 数据段：`seq` 是字节序号（从 0 开始），`len` 为有效载荷字节数
    Data { seq: u64, len: u32 },
    /// ACK 段：`ack` 是期望的下一个字节序号（累计确认）
    Ack { ack: u64 },
}

/// DCTCP 段（极简：只保留实验需要的字段）
#[derive(Debug, Clone)]
pub enum DctcpSegment {
    /// 数据段：`seq` 是字节序号（从 0 开始），`len` 为有效载荷字节数
    Data { seq: u64, len: u32 },
    /// ACK 段：`ack` 是期望的下一个字节序号（累计确认）
    Ack { ack: u64, ecn_echo: bool },
}
