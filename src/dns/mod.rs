//! DNS 提供商模块

mod provider;
mod cloudflare;
mod aliyun;

pub use provider::DNSProvider;
pub use cloudflare::CloudflareProvider;
pub use aliyun::AliyunProvider;
