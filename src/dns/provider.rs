//! DNS 提供商 trait

use std::collections::HashMap;

/// DNS 提供商 trait
#[async_trait::async_trait]
pub trait DNSProvider: Send + Sync {
    /// 创建或更新 DNS AAAA 记录
    async fn upsert_record(
        &self,
        zone: &str,
        record_name: &str,
        ip: &str,
        ttl: u32,
        extra: &HashMap<String, String>,
    ) -> anyhow::Result<()>;
}
