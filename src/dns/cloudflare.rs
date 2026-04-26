//! Cloudflare DNS 提供商

use crate::http::{get_with_retry, post_with_retry};
use crate::utils::logger;
use crate::dns::provider::DNSProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Cloudflare DNS 提供商
pub struct CloudflareProvider {
    api_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CloudflareZoneResponse {
    result: Vec<CloudflareZoneResult>,
    success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CloudflareZoneResult {
    id: String,
    name: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CloudflareDnsResponse {
    result: Vec<CloudflareDnsRecord>,
    success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct CloudflareDnsRecord {
    id: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct CloudflareDnsCreateRequest {
    #[serde(rename = "type")]
    record_type: String,
    name: String,
    content: String,
    ttl: u32,
    proxied: bool,
}

#[derive(Debug, Serialize)]
struct CloudflareDnsUpdateRequest {
    content: String,
    ttl: u32,
    proxied: bool,
}

impl CloudflareProvider {
    pub fn new(api_token: String, _proxy_url: Option<String>) -> anyhow::Result<Self> {
        Ok(Self { api_token })
    }

    /// 获取 Zone ID（公开方法）
    pub async fn get_zone_id(&self, zone_name: &str) -> anyhow::Result<String> {
        let url = format!("https://api.cloudflare.com/client/v4/zones?name={}", zone_name);

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {}", self.api_token));
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let response = get_with_retry(&url, Some(&headers), 15, 2, None).await?;

        if response.status_code != 200 {
            anyhow::bail!("Failed to get zone ID: status={}", response.status_code);
        }

        let cf_response: CloudflareZoneResponse = serde_json::from_str(&response.body)?;

        cf_response
            .result
            .into_iter()
            .find(|z| z.name == zone_name)
            .map(|z| z.id)
            .ok_or_else(|| anyhow::anyhow!("Zone not found: {}", zone_name))
    }

    async fn upsert_record_with_zone_id(
        &self,
        zone_id: &str,
        full_domain: &str,
        ip: &str,
        ttl: u32,
        proxied: bool,
    ) -> anyhow::Result<()> {
        let base_url = "https://api.cloudflare.com/client/v4";

        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), format!("Bearer {}", self.api_token));
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        // 查询现有记录
        let list_url = format!("{}/zones/{}/dns_records?type=AAAA&name={}", base_url, zone_id, full_domain);
        let response = get_with_retry(&list_url, Some(&headers), 15, 2, None).await?;

        let dns_response: CloudflareDnsResponse = serde_json::from_str(&response.body)?;

        let (method, body, record_url) = if dns_response.result.is_empty() {
            // 创建新记录
            let request = CloudflareDnsCreateRequest {
                record_type: "AAAA".to_string(),
                name: full_domain.to_string(),
                content: ip.to_string(),
                ttl,
                proxied,
            };
            ("POST", serde_json::to_string(&request)?, format!("{}/zones/{}/dns_records", base_url, zone_id))
        } else {
            // 更新现有记录
            let request = CloudflareDnsUpdateRequest { content: ip.to_string(), ttl, proxied };
            ("PUT", serde_json::to_string(&request)?, format!("{}/zones/{}/dns_records/{}", base_url, zone_id, dns_response.result[0].id))
        };

        let response = if method == "POST" {
            post_with_retry(&record_url, &body, Some(&headers), 15, 2, None).await?
        } else {
            post_with_retry(&record_url, &body, Some(&headers), 15, 2, None).await?
        };

        if response.status_code != 200 {
            anyhow::bail!("Failed to upsert DNS record: status={}", response.status_code);
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl DNSProvider for CloudflareProvider {
    async fn upsert_record(
        &self,
        zone: &str,
        record_name: &str,
        ip: &str,
        ttl: u32,
        extra: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let zone_id = self.get_zone_id(zone).await?;

        let full_domain = if record_name.is_empty() || record_name == "@" {
            zone.to_string()
        } else {
            format!("{}.{}", record_name, zone)
        };

        let proxied = extra.get("proxied").and_then(|v| v.parse::<bool>().ok()).unwrap_or(false);

        self.upsert_record_with_zone_id(&zone_id, &full_domain, ip, ttl, proxied).await?;

        logger::success(&format!("Cloudflare record {} updated successfully", full_domain));

        Ok(())
    }
}
