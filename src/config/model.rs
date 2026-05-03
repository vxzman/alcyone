//! 配置模型

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 默认配置值
pub const DEFAULT_CLOUDFLARE_TTL: u32 = 180;
pub const DEFAULT_ALIYUN_TTL: u32 = 600;
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 300;

/// IP 来源配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IpSource {
    #[serde(default)]
    pub interface: String,
    #[serde(default)]
    pub urls: Vec<String>,
}

/// 全局配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeneralConfig {
    #[serde(default)]
    pub get_ip: IpSource,
    #[serde(default)]
    pub proxy: String,
}

/// Cloudflare 记录配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CloudflareRecord {
    #[serde(default)]
    pub api_token: String,
    #[serde(default)]
    pub zone_id: String,
    #[serde(default)]
    pub proxied: bool,
    #[serde(default)]
    pub ttl: u32,
}

/// 阿里云记录配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AliyunRecord {
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default)]
    pub access_key_secret: String,
    #[serde(default)]
    pub ttl: u32,
}

/// DNS 记录配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordConfig {
    pub provider: String,
    pub zone: String,
    pub record: String,
    #[serde(default)]
    pub ttl: u32,
    #[serde(default)]
    pub proxied: bool,
    #[serde(default)]
    pub use_proxy: bool,
    #[serde(default)]
    pub cloudflare: Option<CloudflareRecord>,
    #[serde(default)]
    pub aliyun: Option<AliyunRecord>,
}

/// 完整配置文件
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub records: Vec<RecordConfig>,
}
