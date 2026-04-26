//! Alcyone - 动态 DNS 客户端
//! 
//! 一个强大的动态 DNS (DDNS) 客户端，支持多域名、多服务商、IPv6

pub mod utils;
pub mod config;
pub mod cache;
pub mod http;
pub mod ip;
pub mod dns;

// 重新导出常用类型
pub use config::{Config, RecordConfig, ConfigLoader, load_config};
pub use cache::{read_last_ip, write_last_ip, read_zone_id_cache, update_zone_id_cache};
pub use ip::{Ipv6Info, get_from_interface, get_from_apis, select_best};
pub use dns::{DNSProvider, CloudflareProvider, AliyunProvider};
