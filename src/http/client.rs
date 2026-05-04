//! HTTP 客户端

use std::collections::HashMap;
use std::time::Duration;

/// HTTP 响应
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: String,
}

/// HTTP 客户端
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    /// 创建新的 HTTP 客户端
    pub fn new() -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { client })
    }

    /// 创建带代理的 HTTP 客户端
    pub fn with_proxy(proxy_url: &str) -> anyhow::Result<Self> {
        let proxy = reqwest::Proxy::all(proxy_url)?;
        let client = reqwest::Client::builder()
            .proxy(proxy)
            .timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { client })
    }

    /// GET 请求
    pub async fn get(
        &self,
        url: &str,
        headers: Option<&HashMap<String, String>>,
        timeout_secs: u64,
    ) -> anyhow::Result<HttpResponse> {
        let mut request = self.client.get(url);

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                request = request.header(key, value);
            }
        }

        let response = request
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await?;

        let status_code = response.status().as_u16();
        let body = response.text().await?;

        Ok(HttpResponse { status_code, body })
    }

    /// POST 请求
    pub async fn post(
        &self,
        url: &str,
        body: &str,
        headers: Option<&HashMap<String, String>>,
        timeout_secs: u64,
    ) -> anyhow::Result<HttpResponse> {
        let mut request = self.client.post(url).body(body.to_string());

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                request = request.header(key, value);
            }
        }

        let response = request
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await?;

        let status_code = response.status().as_u16();
        let body = response.text().await?;

        Ok(HttpResponse { status_code, body })
    }

    /// PUT 请求
    pub async fn put(
        &self,
        url: &str,
        body: &str,
        headers: Option<&HashMap<String, String>>,
        timeout_secs: u64,
    ) -> anyhow::Result<HttpResponse> {
        let mut request = self.client.put(url).body(body.to_string());

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                request = request.header(key, value);
            }
        }

        let response = request
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await?;

        let status_code = response.status().as_u16();
        let body = response.text().await?;

        Ok(HttpResponse { status_code, body })
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new().expect("Failed to create HTTP client")
    }
}

/// 带重试的 GET 请求
pub async fn get_with_retry(
    url: &str,
    headers: Option<&HashMap<String, String>>,
    timeout_secs: u64,
    max_retries: usize,
    proxy_url: Option<&str>,
) -> anyhow::Result<HttpResponse> {
    let client = if let Some(proxy) = proxy_url {
        HttpClient::with_proxy(proxy)?
    } else {
        HttpClient::new()?
    };

    let mut last_error = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(100 * (1 << attempt))).await;
        }

        match client.get(url, headers, timeout_secs).await {
            Ok(response) => return Ok(response),
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
}

/// 带重试的 POST 请求
pub async fn post_with_retry(
    url: &str,
    body: &str,
    headers: Option<&HashMap<String, String>>,
    timeout_secs: u64,
    max_retries: usize,
    proxy_url: Option<&str>,
) -> anyhow::Result<HttpResponse> {
    let client = if let Some(proxy) = proxy_url {
        HttpClient::with_proxy(proxy)?
    } else {
        HttpClient::new()?
    };

    let mut last_error = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(100 * (1 << attempt))).await;
        }

        match client.post(url, body, headers, timeout_secs).await {
            Ok(response) => return Ok(response),
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
}

/// 带重试的 PUT 请求
pub async fn put_with_retry(
    url: &str,
    body: &str,
    headers: Option<&HashMap<String, String>>,
    timeout_secs: u64,
    max_retries: usize,
    proxy_url: Option<&str>,
) -> anyhow::Result<HttpResponse> {
    let client = if let Some(proxy) = proxy_url {
        HttpClient::with_proxy(proxy)?
    } else {
        HttpClient::new()?
    };

    let mut last_error = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(100 * (1 << attempt))).await;
        }

        match client.put(url, body, headers, timeout_secs).await {
            Ok(response) => return Ok(response),
            Err(e) => {
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error")))
}
