use std::net::Ipv6Addr;

use crate::http;
use crate::utils::logger as log;
use crate::ip::model::*;

/// 检查是否是链路本地地址 (fe80::/10)
fn is_link_local(addr: &Ipv6Addr) -> bool {
    addr.octets()[0] == 0xfe && (addr.octets()[1] & 0xc0) == 0x80
}

/// 检查是否是回环地址 (::1)
#[cfg(target_os = "linux")]
fn is_loopback(addr: &Ipv6Addr) -> bool {
    addr.octets()[0..15].iter().all(|&b| b == 0) && addr.octets()[15] == 1
}

/// 检查是否是唯一本地地址 (ULA, fc00::/7)
fn is_ula(addr: &Ipv6Addr) -> bool {
    addr.octets()[0] == 0xfc || addr.octets()[0] == 0xfd
}

/// 格式化 IPv6 地址
#[cfg(target_os = "linux")]
fn format_ipv6(addr: &Ipv6Addr) -> String {
    addr.to_string()
}

/// 填充 IPv6Info 的额外字段
pub fn populate_info(info: &mut Ipv6Info) {
    if info.ip.is_empty() {
        return;
    }

    let addr: Ipv6Addr = match info.ip.parse() {
        Ok(a) => a,
        Err(_) => return,
    };

    info.is_unique_local = is_ula(&addr);

    // 确定作用域
    info.scope = if is_link_local(&addr) {
        "Link Local".to_string()
    } else if info.is_unique_local {
        "Unique Local (ULA)".to_string()
    } else {
        "Global Unicast".to_string()
    };

    // 确定地址状态
    info.is_deprecated = info.preferred_lft <= 0 && info.valid_lft > 0;

    info.address_state = if info.valid_lft == 0 {
        "Expired".to_string()
    } else if info.is_deprecated {
        "Deprecated".to_string()
    } else if info.preferred_lft < info.valid_lft {
        "Preferred/Dynamic".to_string()
    } else {
        "Preferred/Static".to_string()
    };

    // 判断是否是 DDNS 候选地址
    info.is_candidate = info.scope == "Global Unicast"
        && !info.is_deprecated
        && !info.is_unique_local
        && info.valid_lft > 0;
}

// ============================================================================
// Linux 实现 (使用 rtnetlink)
// ============================================================================

/// Linux Netlink 实现（使用 rtnetlink 库）
#[cfg(target_os = "linux")]
pub async fn get_from_interface(iface_name: &str) -> anyhow::Result<Vec<Ipv6Info>> {
    use futures::TryStreamExt;
    use netlink_packet_route::{
        address::AddressAttribute,
        link::LinkAttribute,
        AddressFamily,
    };

    // 1. 创建 netlink 连接
    let (connection, handle, _) = rtnetlink::new_connection()?;

    // 2. 启动连接（在后台运行）
    tokio::spawn(connection);

    // 3. 获取网卡索引
    let iface_idx = {
        let mut links = handle.link().get().execute();
        let mut idx = None;
        while let Some(msg) = links.try_next().await? {
            let name = msg.attributes.iter().find_map(|attr| {
                if let LinkAttribute::IfName(n) = attr {
                    Some(n)
                } else {
                    None
                }
            });
            if name == Some(&iface_name.to_string()) {
                idx = Some(msg.header.index);
                break;
            }
        }
        idx.ok_or_else(|| anyhow::anyhow!("Interface not found: {}", iface_name))?
    };

    // 4. 获取该网卡的所有地址
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(iface_idx)
        .execute();

    let mut result = Vec::new();
    let mut _seen_any_ipv6 = false;

    while let Some(msg) = addresses.try_next().await? {
        // 检查是否是 IPv6 地址
        if msg.header.family != AddressFamily::Inet6 {
            continue;
        }
        _seen_any_ipv6 = true;

        let mut ipv6_addr: Option<std::net::IpAddr> = None;
        let mut preferred_lft: u32 = 0;
        let mut valid_lft: u32 = 0;

        for attr in msg.attributes {
            match attr {
                AddressAttribute::Address(addr) => {
                    if let std::net::IpAddr::V6(v6) = addr {
                        ipv6_addr = Some(std::net::IpAddr::V6(v6));
                    }
                }
                AddressAttribute::CacheInfo(cache_info) => {
                    // ifa_valid: 有效生命周期(秒), 0xFFFFFFFF = forever
                    // ifa_preferred: 首选生命周期(秒), 0xFFFFFFFF = forever
                    valid_lft = if cache_info.ifa_valid == 0xFFFFFFFF {
                        ND6_INFINITE_LIFETIME
                    } else {
                        cache_info.ifa_valid
                    };
                    preferred_lft = if cache_info.ifa_preferred == 0xFFFFFFFF {
                        ND6_INFINITE_LIFETIME
                    } else {
                        cache_info.ifa_preferred
                    };
                }
                _ => {}
            }
        }

        // 5. 构建 IPv6Info
        if let Some(std::net::IpAddr::V6(addr)) = ipv6_addr {
            // 跳过链路本地和回环地址
            if is_link_local(&addr) || is_loopback(&addr) {
                continue;
            }

            // 跳过已过期的地址
            if valid_lft == 0 {
                continue;
            }

            let is_temporary = addr.octets()[8] & 0x80 != 0;

            let mut info = Ipv6Info {
                ip: format_ipv6(&addr),
                preferred_lft: if preferred_lft == ND6_INFINITE_LIFETIME {
                    INFINITE_LIFETIME_SECONDS
                } else {
                    preferred_lft as i64
                },
                valid_lft: if valid_lft == ND6_INFINITE_LIFETIME {
                    INFINITE_LIFETIME_SECONDS
                } else {
                    valid_lft as i64
                },
                is_deprecated: false,
                is_temporary,
                ..Default::default()
            };

            populate_info(&mut info);

            if info.is_candidate {
                result.push(info);
            }
        }
    }

    if result.is_empty() {
        Err(anyhow::anyhow!(
            "No suitable IPv6 address on interface {}",
            iface_name
        ))
    } else {
        Ok(result)
    }
}

// ============================================================================
// FreeBSD 实现 (使用 ifaddr6 crate)
// ============================================================================

/// FreeBSD 实现 (使用 ifaddr6 crate)
#[cfg(target_os = "freebsd")]
pub async fn get_from_interface(iface_name: &str) -> anyhow::Result<Vec<Ipv6Info>> {
    use ifaddr6::get_addresses;

    let raw_addrs = get_addresses(iface_name).await.map_err(|e| anyhow::anyhow!("{}", e))?;

    let mut result = Vec::new();
    for raw in &raw_addrs {
        let mut info = Ipv6Info {
            ip: raw.address.clone(),
            preferred_lft: if raw.preferred_lft == u32::MAX {
                INFINITE_LIFETIME_SECONDS
            } else {
                raw.preferred_lft as i64
            },
            valid_lft: if raw.valid_lft == u32::MAX {
                INFINITE_LIFETIME_SECONDS
            } else {
                raw.valid_lft as i64
            },
            is_deprecated: false,
            is_temporary: raw.is_temporary,
            ..Default::default()
        };
        populate_info(&mut info);

        if info.is_candidate {
            result.push(info);
        }
    }

    if result.is_empty() {
        Err(anyhow::anyhow!(
            "No suitable IPv6 address on interface {}",
            iface_name
        ))
    } else {
        Ok(result)
    }
}

// ============================================================================
// 非 Linux/FreeBSD 平台使用 API 降级方案 (macOS, OpenBSD, NetBSD, Windows, etc.)
// ============================================================================

/// 非 Linux/FreeBSD 平台使用 API 降级方案
#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
pub async fn get_from_interface(iface_name: &str) -> anyhow::Result<Vec<Ipv6Info>> {
    Err(anyhow::anyhow!(
        "Interface method not supported on this platform, using API fallback for: {}",
        iface_name
    ))
}

/// 从 HTTP API 获取 IPv6 地址（并发查询，返回第一个成功的结果）
pub async fn get_from_apis(urls: &[String]) -> anyhow::Result<Vec<Ipv6Info>> {
    if urls.is_empty() {
        return Err(anyhow::anyhow!("No API URLs configured"));
    }

    // 并发发起所有请求
    let mut handles = Vec::new();

    for url in urls {
        log::info(&format!("Querying API: {}", url));

        let url_clone = url.clone();
        let handle = tokio::spawn(async move {
            match http::get_with_retry(&url_clone, None, 15, 2, None).await {
                Ok(response) => {
                    let ip = response
                        .body
                        .lines()
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    if !ip.is_empty() {
                        Some((url_clone, ip))
                    } else {
                        None
                    }
                }
                Err(_) => None,
            }
        });

        handles.push(handle);
    }

    // 使用 futures::stream 等待第一个成功的结果
    use futures::stream::StreamExt;
    let mut stream = futures::stream::iter(handles)
        .buffer_unordered(urls.len());

    while let Some(result) = stream.next().await {
        match result {
            Ok(Some((url, ip))) => {
                log::info(&format!("API {} succeeded: {}", url, ip));

                let mut info = Ipv6Info {
                    ip,
                    preferred_lft: INFINITE_LIFETIME_SECONDS,
                    valid_lft: INFINITE_LIFETIME_SECONDS,
                    ..Default::default()
                };

                populate_info(&mut info);
                return Ok(vec![info]);
            }
            _ => {
                // 继续等待其他结果
            }
        }
    }

    Err(anyhow::anyhow!("All API requests failed"))
}

/// 选择最佳的 IPv6 地址
/// 优先选择非临时地址，然后选择 preferred_lft 最长的
pub fn select_best(infos: &[Ipv6Info]) -> anyhow::Result<String> {
    let candidates: Vec<&Ipv6Info> = infos.iter().filter(|i| i.is_candidate).collect();

    if candidates.is_empty() {
        return Err(anyhow::anyhow!("No suitable global unicast IPv6 candidate found"));
    }

    // 优先选择非临时地址，然后在同类中选择 preferred_lft 最长的
    let best = candidates
        .iter()
        .max_by_key(|i| {
            // 非临时地址优先 (weight: 2), 临时地址降级 (weight: 1)
            let stability = if i.is_temporary { 1i64 } else { 2i64 };
            stability * 1_000_000_000 + i.preferred_lft
        })
        .unwrap();

    Ok(best.ip.clone())
}
