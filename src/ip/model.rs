//! IP 信息模型

/// IPv6 地址信息
#[derive(Debug, Clone, Default)]
pub struct Ipv6Info {
    pub ip: String,
    pub scope: String,
    pub address_state: String,
    pub preferred_lft: i64,
    pub valid_lft: i64,
    pub is_deprecated: bool,
    pub is_unique_local: bool,
    pub is_candidate: bool,
}

/// 无限生命周期常量
pub const INFINITE_LIFETIME_SECONDS: i64 = 1_000_000_000_000;
pub const ND6_INFINITE_LIFETIME: u32 = 0xFFFFFFFF;
