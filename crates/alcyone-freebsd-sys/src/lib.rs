//! alcyone-freebsd-sys
//!
//! FreeBSD 专用的系统调用封装库，提供 IPv6 地址获取和网卡信息查询功能。
//!
//! ## 架构
//! ```text
//! alcyone (主项目) ──→ alcyone-freebsd-sys (本库) ──→ FreeBSD libc/内核
//! ```
//!
//! ## 使用示例
//! ```ignore
//! use alcyone_freebsd_sys::{fetch_ipv6_lifetimes, fetch_interfaces};
//!
//! let addrs = fetch_ipv6_lifetimes().expect("sysctl failed");
//! let ifaces = fetch_interfaces().expect("getifaddrs failed");
//! ```

use std::collections::HashMap;
use std::ffi::CStr;
use std::net::Ipv6Addr;
use std::ptr;

// ============================================================================
// #[repr(C)] 结构体定义（与 FreeBSD 内核结构体对齐）
// ============================================================================

/// `struct sockaddr_in6` — IPv6 套接字地址
#[repr(C)]
#[derive(Clone, Copy)]
struct SockaddrIn6 {
    sin6_len: u8,
    sin6_family: u8,
    sin6_port: u16,
    sin6_flowinfo: u32,
    sin6_addr: [u8; 16],
    sin6_scope_id: u32,
}

/// `struct ifaddrs` — 网络接口地址链表节点
#[repr(C)]
struct IfAddrs {
    ifa_next: *mut IfAddrs,
    ifa_name: *mut libc::c_char,
    ifa_flags: libc::c_uint,
    ifa_addr: *mut libc::sockaddr,
    ifa_netmask: *mut libc::sockaddr,
    ifa_dstaddr: *mut libc::sockaddr,
    ifa_data: *mut libc::c_void,
}

/// `struct nd6_lifetime` — ND6 生命周期
#[repr(C)]
#[derive(Clone, Copy)]
struct Nd6Lifetime {
    ntl_preferred: u32,
    ntl_valid: u32,
}

/// `struct in6_ifaddr`（精简版，仅包含需要的字段）
#[repr(C)]
struct In6IfAddr {
    ia_addr: SockaddrIn6,
    ia_netmask: *mut SockaddrIn6,
    ia_dstaddr: *mut SockaddrIn6,
    _pad: [u8; 16],
    ia6_lifetime: Nd6Lifetime,
}

// ============================================================================
// extern "C" 声明（直接链接 FreeBSD libc）
// ============================================================================

extern "C" {
    fn getifaddrs(ifap: *mut *mut IfAddrs) -> libc::c_int;
    fn freeifaddrs(ifa: *mut IfAddrs);
}

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

// ============================================================================
// RAII 守卫
// ============================================================================

struct IfAddrsGuard(*mut IfAddrs);

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
// 公开 API — 纯 Rust 返回类型
// ============================================================================

/// 单个 IPv6 地址及其生命周期（从 `sysctl` 获取）
#[derive(Debug, Clone)]
pub struct Ipv6AddrEntry {
    /// IPv6 地址
    pub addr: Ipv6Addr,
    /// 首选生命周期（秒，`u32::MAX` 表示无限）
    pub preferred_lft: u32,
    /// 有效生命周期（秒，`u32::MAX` 表示无限）
    pub valid_lft: u32,
}

/// 网络接口信息（从 `getifaddrs` 获取）
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    /// 接口名称（如 `em0`, `igb0`）
    pub name: String,
    /// 是否是 IPv6 地址
    pub is_ipv6: bool,
    /// IPv6 地址（如果适用）
    pub ipv6_addr: Option<Ipv6Addr>,
    /// 接口标志（`ifa_flags` 原始值）
    pub flags: libc::c_uint,
}

/// 通过 `sysctl(CTL_NET, AF_INET6, IPPROTO_IPV6, IPV6CTL_ADDRLIST)` 获取所有 IPv6 地址及生命周期
pub fn fetch_ipv6_lifetimes() -> std::io::Result<Vec<Ipv6AddrEntry>> {
    // MIB: CTL_NET → AF_INET6 → IPPROTO_IPV6 → IPV6CTL_ADDRLIST(22)
    let mut mib = [
        libc::CTL_NET as i32,
        libc::AF_INET6 as i32,
        libc::IPPROTO_IPV6 as i32,
        22, // IPV6CTL_ADDRLIST
    ];

    // 第一次调用：获取缓冲区大小
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
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "sysctl IPV6CTL_ADDRLIST query size failed",
        ));
    }

    // 第二次调用：获取实际数据
    let mut buf = vec![0u8; buf_size];
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
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "sysctl IPV6CTL_ADDRLIST fetch failed",
        ));
    }

    // 解析 in6_ifaddr 结构体数组
    let mut entries = Vec::new();
    let mut offset = 0;

    while offset + std::mem::size_of::<In6IfAddr>() <= buf.len() {
        let in6ifa = unsafe { &*(buf.as_ptr().add(offset) as *const In6IfAddr) };

        let addr = Ipv6Addr::from(in6ifa.ia_addr.sin6_addr);
        let preferred_lft = in6ifa.ia6_lifetime.ntl_preferred;
        let valid_lft = in6ifa.ia6_lifetime.ntl_valid;

        // 全零表示链表结束
        if preferred_lft == 0 && valid_lft == 0 {
            break;
        }

        entries.push(Ipv6AddrEntry {
            addr,
            preferred_lft,
            valid_lft,
        });

        offset += std::mem::size_of::<In6IfAddr>();
    }

    Ok(entries)
}

/// 通过 `getifaddrs()` 获取所有网络接口及其 IPv6 地址
pub fn fetch_interfaces() -> std::io::Result<Vec<InterfaceInfo>> {
    let mut ifap: *mut IfAddrs = ptr::null_mut();
    let ret = unsafe { getifaddrs(&mut ifap) };
    if ret != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let _guard = IfAddrsGuard(ifap);

    let mut result = Vec::new();
    let mut current = ifap;

    unsafe {
        while !current.is_null() {
            let ifa = &*current;

            // 读取网卡名
            let name = if !ifa.ifa_name.is_null() {
                CStr::from_ptr(ifa.ifa_name)
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::from("(unknown)")
            };

            // 检查是否为 IPv6 地址
            let (is_ipv6, ipv6_addr) = if !ifa.ifa_addr.is_null() {
                let sa = &*ifa.ifa_addr;
                if sa.sa_family as i32 == libc::AF_INET6 {
                    let sin6 = &*(ifa.ifa_addr as *const SockaddrIn6);
                    (true, Some(Ipv6Addr::from(sin6.sin6_addr)))
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            };

            result.push(InterfaceInfo {
                name,
                is_ipv6,
                ipv6_addr,
                flags: ifa.ifa_flags,
            });

            current = ifa.ifa_next;
        }
    }

    Ok(result)
}

/// 便捷函数：将 IPv6 地址列表和接口列表合并，按接口名分组
pub fn resolve_ipv6_by_interface(
    entries: &[Ipv6AddrEntry],
    interfaces: &[InterfaceInfo],
) -> HashMap<String, Vec<Ipv6AddrEntry>> {
    let mut map = HashMap::new();

    // 构建 (iface_name, ipv6_addr) → entry 的映射
    for iface in interfaces {
        if let Some(addr) = iface.ipv6_addr {
            if let Some(entry) = entries.iter().find(|e| e.addr == addr) {
                map
                    .entry(iface.name.clone())
                    .or_insert_with(Vec::new)
                    .push(entry.clone());
            }
        }
    }

    map
}
