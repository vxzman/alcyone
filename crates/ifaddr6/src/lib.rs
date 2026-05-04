//! ifaddr6 - Cross-platform IPv6 address discovery with lifetime information
//!
//! ## Features
//!
//! - FreeBSD: `getifaddrs()` + `ioctl(SIOCGIFALIFETIME_IN6)`
//! - Linux: (placeholder — stub, falls through to caller's fallback)
//! - Detects and reports temporary (privacy extension) addresses
//! - Filters link-local, loopback, and ULA addresses by default
//!
//! ## Example
//!
//! ```ignore
//! use ifaddr6::{Ipv6AddrInfo, get_addresses};
//!
//! let addrs = get_addresses("eth0").await?;
//!
//! for addr in addrs {
//!     println!("{} preferred={}s valid={}s temp={}",
//!         addr.address, addr.preferred_lft, addr.valid_lft, addr.is_temporary);
//! }
//! ```

use thiserror::Error;

/// Errors from ifaddr6 operations.
#[derive(Debug, Error)]
pub enum IfAddr6Error {
    #[error("interface not found: {0}")]
    InterfaceNotFound(String),

    #[error("system error: {0}")]
    SystemError(String),

    #[error("no IPv6 addresses found on interface: {0}")]
    NoAddresses(String),

    #[error("platform not supported")]
    Unsupported,
}

/// A single IPv6 address discovered on an interface.
#[derive(Debug, Clone)]
pub struct Ipv6AddrInfo {
    /// The IPv6 address string (e.g. `"2409:8a6c::1"`)
    pub address: String,
    /// Interface name (e.g. `"vtnet0"`)
    pub interface: String,
    /// Preferred lifetime in seconds.
    /// `u32::MAX` means infinite/never expires.
    pub preferred_lft: u32,
    /// Valid lifetime in seconds.
    /// `u32::MAX` means infinite/never expires.
    pub valid_lft: u32,
    /// Whether this is a temporary/privacy extension address (RFC 4941).
    pub is_temporary: bool,
}

// ============================================================================
// FreeBSD implementation
// ============================================================================

#[cfg(target_os = "freebsd")]
mod freebsd {
    use super::*;

    const MAX_ADDRS: usize = 64;

    #[repr(C)]
    struct RawEntry {
        addr: [std::os::raw::c_char; 46],
        iface: [std::os::raw::c_char; 16],
        preferred_lft: u32,
        valid_lft: u32,
        is_temporary: u8,
    }

    extern "C" {
        fn ifaddr6_query(
            ifname: *const std::os::raw::c_char,
            results: *mut RawEntry,
            max_results: std::os::raw::c_int,
            error_code: *mut std::os::raw::c_int,
        ) -> std::os::raw::c_int;
    }

    pub(crate) fn query(iface: &str) -> Result<Vec<Ipv6AddrInfo>, IfAddr6Error> {
        let c_ifname = std::ffi::CString::new(iface)
            .map_err(|e| IfAddr6Error::SystemError(e.to_string()))?;

        let mut results: [RawEntry; MAX_ADDRS] =
            unsafe { std::mem::zeroed() };
        let mut error_code: std::os::raw::c_int = 0;

        let count = unsafe {
            ifaddr6_query(
                c_ifname.as_ptr(),
                results.as_mut_ptr(),
                MAX_ADDRS as std::os::raw::c_int,
                &mut error_code,
            )
        };

        if count < 0 {
            return match error_code {
                1 => Err(IfAddr6Error::InterfaceNotFound(iface.to_string())),
                _ => Err(IfAddr6Error::SystemError(format!(
                    "error code: {}",
                    error_code
                ))),
            };
        }

        let mut infos = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let r = &results[i];
            let address = unsafe { std::ffi::CStr::from_ptr(r.addr.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            let interface =
                unsafe { std::ffi::CStr::from_ptr(r.iface.as_ptr()) }
                    .to_string_lossy()
                    .into_owned();

            infos.push(Ipv6AddrInfo {
                address,
                interface,
                preferred_lft: r.preferred_lft,
                valid_lft: r.valid_lft,
                is_temporary: r.is_temporary != 0,
            });
        }

        if infos.is_empty() {
            Err(IfAddr6Error::NoAddresses(iface.to_string()))
        } else {
            Ok(infos)
        }
    }
}

// ============================================================================
// Linux stub (placeholder — not yet implemented)
// ============================================================================

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    pub(crate) fn query(_iface: &str) -> Result<Vec<Ipv6AddrInfo>, IfAddr6Error> {
        // TODO: implement via rtnetlink
        Err(IfAddr6Error::Unsupported)
    }
}

// ============================================================================
// Public API
// ============================================================================

/// Query IPv6 addresses on the given interface.
///
/// Returns addresses sorted by `preferred_lft` descending (longest first),
/// with non-temporary addresses preferred over temporary ones.
///
/// On FreeBSD, uses `getifaddrs()` + `ioctl(SIOCGIFALIFETIME_IN6)`.
/// On other platforms, returns `IfAddr6Error::Unsupported`.
pub async fn get_addresses(iface: &str) -> Result<Vec<Ipv6AddrInfo>, IfAddr6Error> {
    #[cfg(target_os = "freebsd")]
    {
        freebsd::query(iface)
    }

    #[cfg(target_os = "linux")]
    {
        linux::query(iface)
    }

    #[cfg(not(any(target_os = "freebsd", target_os = "linux")))]
    {
        Err(IfAddr6Error::Unsupported)
    }
}

/// Select the best (non-temporary, longest preferred lifetime) address.
pub fn select_best(addresses: &[Ipv6AddrInfo]) -> Option<&Ipv6AddrInfo> {
    addresses
        .iter()
        // Filter out temporary addresses first, fall back to all if none remain
        .filter(|a| !a.is_temporary && a.valid_lft > 0)
        .collect::<Vec<_>>()
        .first()
        .copied()
        .or_else(|| addresses.iter().max_by_key(|a| a.preferred_lft))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_best_prefers_non_temporary() {
        let addrs = vec![
            Ipv6AddrInfo {
                address: "2409::temp".to_string(),
                interface: "eth0".to_string(),
                preferred_lft: 100000,
                valid_lft: 200000,
                is_temporary: true,
            },
            Ipv6AddrInfo {
                address: "2409::stable".to_string(),
                interface: "eth0".to_string(),
                preferred_lft: 50000,
                valid_lft: 100000,
                is_temporary: false,
            },
        ];
        let best = select_best(&addrs).unwrap();
        assert_eq!(best.address, "2409::stable");
    }

    #[test]
    fn test_select_best_fallback_to_temp_when_no_stable() {
        let addrs = vec![
            Ipv6AddrInfo {
                address: "2409::temp1".to_string(),
                interface: "eth0".to_string(),
                preferred_lft: 100000,
                valid_lft: 200000,
                is_temporary: true,
            },
        ];
        let best = select_best(&addrs).unwrap();
        assert_eq!(best.address, "2409::temp1");
    }
}
