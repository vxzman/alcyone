//! 阿里云 DNS 提供商

use crate::http::get_with_retry;
use crate::utils::logger;
use crate::dns::provider::DNSProvider;
use base64::Engine;
use chrono::{Utc, Timelike};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::HashMap;

type HmacSha1 = Hmac<Sha1>;

/// 阿里云 DNS 提供商
pub struct AliyunProvider {
    access_key_id: String,
    access_key_secret: String,
}

impl AliyunProvider {
    pub fn new(access_key_id: String, access_key_secret: String) -> Self {
        Self {
            access_key_id,
            access_key_secret,
        }
    }

    fn url_encode(&self, s: &str) -> String {
        percent_encoding::percent_encode(s.as_bytes(), percent_encoding::NON_ALPHANUMERIC)
            .to_string()
            .replace("+", "%20")
            .replace("*", "%2A")
            .replace("%7E", "~")
    }

    fn hmac_sha1_base64(&self, key: &str, data: &str) -> String {
        let mut mac = HmacSha1::new_from_slice(key.as_bytes()).expect("HMAC can take key of any size");
        mac.update(data.as_bytes());
        let result = mac.finalize();
        base64::engine::general_purpose::STANDARD.encode(result.into_bytes())
    }

    fn generate_signature(&self, params: &HashMap<String, String>) -> String {
        let mut sorted_keys: Vec<&String> = params.keys().collect();
        sorted_keys.sort();

        let canonical_query_string: String = sorted_keys
            .iter()
            .map(|k| format!("{}={}", self.url_encode(k), self.url_encode(&params[*k])))
            .collect::<Vec<_>>()
            .join("&");

        let string_to_sign = format!("GET&{}&{}", self.url_encode("/"), self.url_encode(&canonical_query_string));

        let key = format!("{}&", self.access_key_secret);
        self.hmac_sha1_base64(&key, &string_to_sign)
    }

    fn build_common_params(&self) -> HashMap<String, String> {
        let mut params = HashMap::new();
        params.insert("Format".to_string(), "JSON".to_string());
        params.insert("Version".to_string(), "2015-01-09".to_string());
        params.insert("AccessKeyId".to_string(), self.access_key_id.clone());
        params.insert("SignatureMethod".to_string(), "HMAC-SHA1".to_string());
        params.insert("Timestamp".to_string(), Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
        params.insert("SignatureVersion".to_string(), "1.0".to_string());
        params.insert("SignatureNonce".to_string(), format!("{:016x}", chrono::Utc::now().timestamp() as u64 * 1000000 + chrono::Utc::now().nanosecond() as u64));
        params
    }

    async fn sign_and_request(&self, mut params: HashMap<String, String>) -> anyhow::Result<crate::http::HttpResponse> {
        let common_params = self.build_common_params();
        for (k, v) in common_params {
            params.insert(k, v);
        }

        let signature = self.generate_signature(&params);
        params.insert("Signature".to_string(), signature);

        let query_string: String = params
            .iter()
            .map(|(k, v)| format!("{}={}", self.url_encode(k), self.url_encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let url = format!("https://alidns.aliyuncs.com/?{}", query_string);
        get_with_retry(&url, None, 15, 2, None).await
    }

    async fn get_record_id(&self, full_domain: &str) -> anyhow::Result<Option<String>> {
        let mut params = HashMap::new();
        params.insert("Action".to_string(), "DescribeDomainRecords".to_string());
        params.insert("DomainKey".to_string(), full_domain.to_string());
        params.insert("TypeKey".to_string(), "AAAA".to_string());

        let response = self.sign_and_request(params).await?;

        if response.status_code != 200 {
            anyhow::bail!("Failed to describe domain records: status={}", response.status_code);
        }

        let json: serde_json::Value = serde_json::from_str(&response.body)?;

        if let Some(records) = json.get("DomainRecords").and_then(|r| r.get("Record")) {
            if let Some(record_list) = records.as_array() {
                for record in record_list {
                    if let (Some(name), Some(id), Some(rtype)) = (
                        record.get("RR").and_then(|v| v.as_str()),
                        record.get("RecordId").and_then(|v| v.as_str()),
                        record.get("Type").and_then(|v| v.as_str()),
                    ) {
                        if rtype == "AAAA" && name == full_domain {
                            return Ok(Some(id.to_string()));
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

#[async_trait::async_trait]
impl DNSProvider for AliyunProvider {
    async fn upsert_record(
        &self,
        zone: &str,
        record_name: &str,
        ip: &str,
        ttl: u32,
        _extra: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let full_domain = if record_name.is_empty() || record_name == "@" {
            zone.to_string()
        } else {
            format!("{}.{}", record_name, zone)
        };

        let record_id = self.get_record_id(&full_domain).await?;

        let mut params = HashMap::new();

        if let Some(id) = record_id {
            params.insert("Action".to_string(), "UpdateDomainRecord".to_string());
            params.insert("RecordId".to_string(), id);
            params.insert("RR".to_string(), record_name.to_string());
            params.insert("Type".to_string(), "AAAA".to_string());
            params.insert("Value".to_string(), ip.to_string());
            params.insert("TTL".to_string(), ttl.to_string());
        } else {
            params.insert("Action".to_string(), "AddDomainRecord".to_string());
            params.insert("DomainName".to_string(), zone.to_string());
            params.insert("RR".to_string(), record_name.to_string());
            params.insert("Type".to_string(), "AAAA".to_string());
            params.insert("Value".to_string(), ip.to_string());
            params.insert("TTL".to_string(), ttl.to_string());
        }

        let response = self.sign_and_request(params).await?;

        if response.status_code != 200 {
            anyhow::bail!("Failed to upsert DNS record: status={}", response.status_code);
        }

        let json: serde_json::Value = serde_json::from_str(&response.body)?;
        if let Some(code) = json.get("Code").and_then(|v| v.as_str()) {
            if code != "" {
                let message = json.get("Message").and_then(|v| v.as_str()).unwrap_or("Unknown error");
                anyhow::bail!("Aliyun API error: {} - {}", code, message);
            }
        }

        logger::success(&format!("Aliyun record {} updated successfully", full_domain));

        Ok(())
    }
}
