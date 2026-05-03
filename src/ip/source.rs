use std::net::Ipv6Addr;

use crate::http;
use crate::utils::logger as log;
use crate::ip::model::*;

/// 检查是否是链路本地地址 (fe80::/10)
fn is_link_local(addr: &Ipv6Addr) -> bool {
    addr.octets()[0] == 0xfe && (addr.octets()[1] & 0xc0) == 0x80
}

/// 检查是否是回环地址 (::1)
fn is_loopback(addr: &Ipv6Addr) -> bool {
    addr.octets()[0..15].iter().all(|&b| b == 0) && addr.octets()[15] == 1
}

/// 检查是否是唯一本地地址 (ULA, fc00::/7)
fn is_ula(addr: &Ipv6Addr) -> bool {
    addr.octets()[0] == 0xfc || addr.octets()[0] == 0xfd
}

/// 格式化 IPv6 地址
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
    use rtnetlink::new_connection;
    use futures::TryStreamExt;

    // 1. 创建 netlink 连接
    let (connection, handle, _) = new_connection()?;
    
    // 2. 启动连接（在后台运行）
    tokio::spawn(connection);

    // 3. 获取网卡索引（使用 match_name）
    let iface_idx = get_interface_index(&handle, iface_name).await?;

    // 4. 获取该网卡的所有地址
    let mut addresses = handle
        .address()
        .get()
        .set_link_index_filter(iface_idx)
        .execute();

    let mut result = Vec::new();

    while let Some(msg) = addresses.try_next().await? {
        // 检查是否是 IPv6 地址（2 = AF_INET6）
        if msg.header.family != 2 {
            continue;
        }

        // 5. 解析地址属性（使用 nlas 字段）
        let mut ipv6_addr: Option<Ipv6Addr> = None;
        let mut preferred_lft: u32 = 0;
        let mut valid_lft: u32 = 0;

        use netlink_packet_route::nlas::address::Nla;
        for attr in msg.nlas {
            match attr {
                Nla::Address(addr) | Nla::Local(addr)
                    if addr.len() == 16 => {
                        let mut bytes = [0u8; 16];
                        bytes.copy_from_slice(&addr);
                        ipv6_addr = Some(Ipv6Addr::from(bytes));
                    }
                Nla::CacheInfo(cache)
                    // cache 是 Vec<u8>, 需要手动解析 ifa_cacheinfo 结构
                    // ifa_cacheinfo: 4 个 u32 (preferred, valid, cstamp, tstamp)
                    if cache.len() >= 16 => {
                        preferred_lft = u32::from_ne_bytes([cache[0], cache[1], cache[2], cache[3]]);
                        valid_lft = u32::from_ne_bytes([cache[4], cache[5], cache[6], cache[7]]);
                    }
                _ => {}
            }
        }

        let flags = msg.header.flags;

        // 6. 构建 IPv6Info
        if let Some(addr) = ipv6_addr {
            // 跳过链路本地和回环地址
            if is_link_local(&addr) || is_loopback(&addr) {
                continue;
            }

            // 跳过已过期的地址
            if valid_lft == 0 {
                continue;
            }

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
                is_deprecated: (flags as u32 & 0x20) != 0, // IFA_F_DEPRECATED
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

/// 获取网卡索引
#[cfg(target_os = "linux")]
async fn get_interface_index(handle: &rtnetlink::Handle, iface_name: &str) -> anyhow::Result<u32> {
    use futures::TryStreamExt;

    // 使用 match_name 过滤
    let mut links = handle
        .link()
        .get()
        .match_name(iface_name.to_string())
        .execute();
    
    if let Some(msg) = links.try_next().await? {
        return Ok(msg.header.index);
    }

    Err(anyhow::anyhow!("Interface not found: {}", iface_name))
}

// ============================================================================
// FreeBSD 实现 (使用 sysctl 获取 IPv6 地址和生命周期)
// ============================================================================

/// FreeBSD 使用 sysctl 实现
#[cfg(target_os = "freebsd")]
pub async fn get_from_interface(iface_name: &str) -> anyhow::Result<Vec<Ipv6Info>> {
    use std::ffi::CStr;
    use std::ptr;
    use std::slice;

    // 定义 sockaddr_in6 结构
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct sockaddr_in6 {
        sin6_len: u8,
        sin6_family: u8,
        sin6_port: u16,
        sin6_flowinfo: u32,
        sin6_addr: [u8; 16],
        sin6_scope_id: u32,
    }

    // 定义 nd6_lifetime 结构
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct nd6_lifetime {
        ntl_preferred: u32,
        ntl_valid: u32,
    }

    // 定义 in6_ifaddr 结构（简化版，只包含我们需要的字段）
    #[repr(C)]
    struct in6_ifaddr {
        ia_addr: sockaddr_in6,
        ia_netmask: *mut sockaddr_in6,
        ia_dstaddr: *mut sockaddr_in6,
        _pad: [u8; 16],  // 填充，跳过一些不需要的字段
        ia6_lifetime: nd6_lifetime,
    }

    // 定义 ifaddrs 结构（用于获取网卡名称）
    #[repr(C)]
    struct ifaddrs {
        ifa_next: *mut ifaddrs,
        ifa_name: *mut libc::c_char,
        ifa_flags: libc::c_uint,
        ifa_addr: *mut libc::sockaddr,
        ifa_netmask: *mut libc::sockaddr,
        ifa_dstaddr: *mut libc::sockaddr,
        ifa_data: *mut libc::c_void,
    }

    // sysctl 外部函数声明
    extern "C" {
        fn sysctl(
            name: *mut libc::c_int,
            namelen: libc::c_uint,
            oldp: *mut libc::c_void,
            oldlenp: *mut libc::size_t,
            newp: *mut libc::c_void,
            newlen: libc::size_t,
        ) -> libc::c_int;
    }

    // getifaddrs 外部函数声明
    extern "C" {
        fn getifaddrs(ifap: *mut *mut ifaddrs) -> libc::c_int;
        fn freeifaddrs(ifa: *mut ifaddrs);
    }

    // ========================================================================
    // 步骤 1: 使用 sysctl 获取 IPv6 地址列表（包含生命周期）
    // ========================================================================

    // 构建 sysctl MIB: CTL_NET.AF_INET6.IPPROTO_IPV6.IPV6CTL_ADDRLIST
    let mut mib = [
        libc::CTL_NET as i32,
        libc::AF_INET6 as i32,
        libc::IPPROTO_IPV6 as i32,
        22 as i32,  // IPV6CTL_ADDRLIST
    ];

    // 第一次调用 sysctl 获取缓冲区大小
    let mut buf_size: libc::size_t = 0;
    let ret = unsafe {
        sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            ptr::null_mut(),
            &mut buf_size,
            ptr::null_mut(),
            0,
        )
    };
    
    if ret != 0 || buf_size == 0 {
        // sysctl 失败，降级到 getifaddrs
        log::info!("sysctl IPV6CTL_ADDRLIST not available, using getifaddrs fallback");
        return get_from_interface_fallback(iface_name);
    }

    // 分配缓冲区
    let mut buf = vec![0u8; buf_size];
    
    // 第二次调用获取实际数据
    let ret = unsafe {
        sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            buf.as_mut_ptr() as *mut libc::c_void,
            &mut buf_size,
            ptr::null_mut(),
            0,
        )
    };
    
    if ret != 0 {
        log::info!("sysctl IPV6CTL_ADDRLIST failed, using getifaddrs fallback");
        return get_from_interface_fallback(iface_name);
    }

    // ========================================================================
    // 步骤 2: 解析 sysctl 返回的数据
    // ========================================================================
    
    // sysctl 返回的是 in6_ifaddr 结构链表
    let mut ipv6_map: std::collections::HashMap<String, (u32, u32)> = std::collections::HashMap::new();
    
    // 遍历缓冲区，解析 in6_ifaddr 结构
    let mut offset = 0;
    while offset + std::mem::size_of::<in6_ifaddr>() <= buf.len() {
        let ifa_ptr = unsafe {
            (buf.as_ptr().add(offset) as *const in6_ifaddr)
        };
        
        let ifa = unsafe { &*ifa_ptr };
        
        // 提取 IPv6 地址
        let ipv6_addr = Ipv6Addr::from(ifa.ia_addr.sin6_addr);
        let ip_str = format_ipv6(&ipv6_addr);
        
        // 提取生命周期
        let preferred_lft = ifa.ia6_lifetime.ntl_preferred;
        let valid_lft = ifa.ia6_lifetime.ntl_valid;
        
        // 存储到映射中
        ipv6_map.insert(ip_str, (preferred_lft, valid_lft));
        
        // 移动到下一个结构
        offset += std::mem::size_of::<in6_ifaddr>();
        
        // 如果生命周期为 0，可能是链表结束
        if valid_lft == 0 && preferred_lft == 0 {
            break;
        }
    }

    // ========================================================================
    // 步骤 3: 使用 getifaddrs 获取网卡名称关联
    // ========================================================================
    
    let mut ifap: *mut ifaddrs = ptr::null_mut();
    let ret = unsafe { getifaddrs(&mut ifap) };
    if ret != 0 {
        return Err(anyhow::anyhow!("getifaddrs failed: {}", std::io::Error::last_os_error()));
    }
    
    let _guard = IfAddrsGuard(ifap);
    
    let mut result = Vec::new();
    let mut ifa = ifap;
    
    unsafe {
        while !ifa.is_null() {
            let ifa_ref = &*ifa;
            
            // 检查网卡名称
            let matches_name = if !ifa_ref.ifa_name.is_null() {
                let name_cstr = CStr::from_ptr(ifa_ref.ifa_name);
                let name = name_cstr.to_string_lossy();
                name == iface_name
            } else {
                false
            };
            
            if !matches_name {
                ifa = (*ifa).ifa_next;
                continue;
            }
            
            // 检查地址族
            if !ifa_ref.ifa_addr.is_null() {
                let addr = &*ifa_ref.ifa_addr;
                if addr.sa_family as i32 != libc::AF_INET6 {
                    ifa = (*ifa).ifa_next;
                    continue;
                }
                
                // 转换为 sockaddr_in6
                let sin6 = &*(ifa_ref.ifa_addr as *const sockaddr_in6);
                let ipv6_addr = Ipv6Addr::from(sin6.sin6_addr);
                
                // 跳过链路本地地址
                if is_link_local(&ipv6_addr) {
                    ifa = (*ifa).ifa_next;
                    continue;
                }
                
                // 跳过回环地址
                if is_loopback(&ipv6_addr) {
                    ifa = (*ifa).ifa_next;
                    continue;
                }
                
                // 跳过 ULA
                if is_ula(&ipv6_addr) {
                    ifa = (*ifa).ifa_next;
                    continue;
                }
                
                // 检查网卡是否运行中
                if (ifa_ref.ifa_flags & (libc::IFF_RUNNING as libc::c_uint)) == 0 {
                    ifa = (*ifa).ifa_next;
                    continue;
                }
                
                // 从映射中获取生命周期
                let ip_str = format_ipv6(&ipv6_addr);
                let (preferred_lft, valid_lft) = ipv6_map.get(&ip_str)
                    .copied()
                    .unwrap_or((ND6_INFINITE_LIFETIME, ND6_INFINITE_LIFETIME));
                
                let mut info = Ipv6Info {
                    ip: ip_str,
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
                    is_deprecated: preferred_lft == 0 && valid_lft > 0,
                    ..Default::default()
                };
                
                populate_info(&mut info);
                
                if info.is_candidate {
                    result.push(info);
                }
            }
            
            ifa = (*ifa).ifa_next;
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

/// FreeBSD 降级方案（仅使用 getifaddrs，无生命周期）
#[cfg(target_os = "freebsd")]
fn get_from_interface_fallback(iface_name: &str) -> anyhow::Result<Vec<Ipv6Info>> {
    use std::ffi::CStr;
    use std::ptr;

    // 定义 sockaddr_in6 结构
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct sockaddr_in6 {
        sin6_len: u8,
        sin6_family: u8,
        sin6_port: u16,
        sin6_flowinfo: u32,
        sin6_addr: [u8; 16],
        sin6_scope_id: u32,
    }

    // 定义 ifaddrs 结构
    #[repr(C)]
    struct ifaddrs {
        ifa_next: *mut ifaddrs,
        ifa_name: *mut libc::c_char,
        ifa_flags: libc::c_uint,
        ifa_addr: *mut libc::sockaddr,
        ifa_netmask: *mut libc::sockaddr,
        ifa_dstaddr: *mut libc::sockaddr,
        ifa_data: *mut libc::c_void,
    }

    // 外部函数声明
    extern "C" {
        fn getifaddrs(ifap: *mut *mut ifaddrs) -> libc::c_int;
        fn freeifaddrs(ifa: *mut ifaddrs);
    }

    let mut ifap: *mut ifaddrs = ptr::null_mut();
    let ret = unsafe { getifaddrs(&mut ifap) };
    if ret != 0 {
        return Err(anyhow::anyhow!("getifaddrs failed: {}", std::io::Error::last_os_error()));
    }

    let _guard = IfAddrsGuard(ifap);
    let mut result = Vec::new();
    let mut ifa = ifap;

    unsafe {
        while !ifa.is_null() {
            let ifa_ref = &*ifa;

            // 检查网卡名称
            if !ifa_ref.ifa_name.is_null() {
                let name_cstr = CStr::from_ptr(ifa_ref.ifa_name);
                let name = name_cstr.to_string_lossy();
                if name != iface_name {
                    ifa = (*ifa).ifa_next;
                    continue;
                }
            }

            // 检查地址族
            if !ifa_ref.ifa_addr.is_null() {
                let addr = &*ifa_ref.ifa_addr;
                if addr.sa_family as i32 != libc::AF_INET6 {
                    ifa = (*ifa).ifa_next;
                    continue;
                }

                // 转换为 sockaddr_in6
                let sin6 = &*(ifa_ref.ifa_addr as *const sockaddr_in6);
                let ipv6_addr = Ipv6Addr::from(sin6.sin6_addr);

                // 跳过链路本地地址
                if is_link_local(&ipv6_addr) {
                    ifa = (*ifa).ifa_next;
                    continue;
                }

                // 跳过回环地址
                if is_loopback(&ipv6_addr) {
                    ifa = (*ifa).ifa_next;
                    continue;
                }

                // 跳过 ULA
                if is_ula(&ipv6_addr) {
                    ifa = (*ifa).ifa_next;
                    continue;
                }

                // 检查网卡是否运行中
                if (ifa_ref.ifa_flags & (libc::IFF_RUNNING as libc::c_uint)) == 0 {
                    ifa = (*ifa).ifa_next;
                    continue;
                }

                // 使用默认生命周期
                let mut info = Ipv6Info {
                    ip: format_ipv6(&ipv6_addr),
                    preferred_lft: INFINITE_LIFETIME_SECONDS,
                    valid_lft: INFINITE_LIFETIME_SECONDS,
                    is_deprecated: false,
                    ..Default::default()
                };

                populate_info(&mut info);

                if info.is_candidate {
                    result.push(info);
                }
            }

            ifa = (*ifa).ifa_next;
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

#[cfg(target_os = "freebsd")]
struct IfAddrsGuard(*mut ifaddrs);

#[cfg(target_os = "freebsd")]
impl Drop for IfAddrsGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                freeifaddrs(self.0);
            }
        }
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

/// 从 HTTP API 获取 IPv6 地址
pub async fn get_from_apis(urls: &[String]) -> anyhow::Result<Vec<Ipv6Info>> {
    if urls.is_empty() {
        return Err(anyhow::anyhow!("No API URLs configured"));
    }

    let mut last_error = None;

    for url in urls {
        log::info(&format!("Querying API: {}", url));

        match http::get_with_retry(url, None, 15, 2, None).await {
            Ok(response) => {
                // 解析响应：提取第一行并去除空白
                let ip = response
                    .body
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();

                if !ip.is_empty() {
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

                log::error(&format!("API {} returned empty response", url));
            }
            Err(e) => {
                log::error(&format!("API {} failed: {}", url, e));
                last_error = Some(e);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("All API requests failed")))
}

/// 选择最佳的 IPv6 地址（优先选择 preferred_lft 最长的）
pub fn select_best(infos: &[Ipv6Info]) -> anyhow::Result<String> {
    let candidates: Vec<&Ipv6Info> = infos.iter().filter(|i| i.is_candidate).collect();

    if candidates.is_empty() {
        return Err(anyhow::anyhow!("No suitable global unicast IPv6 candidate found"));
    }

    // 选择 preferred_lft 最长的地址
    let best = candidates
        .iter()
        .max_by_key(|i| i.preferred_lft)
        .unwrap();

    Ok(best.ip.clone())
}
