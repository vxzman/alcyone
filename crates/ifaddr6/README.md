# ifaddr6

Cross-platform IPv6 address discovery with lifetime information.

## Features

- **FreeBSD**: `getifaddrs()` + `ioctl(SIOCGIFALIFETIME_IN6)` + `ioctl(SIOCGIFAFLAG_IN6)`
- **Linux**: (coming soon — rtnetlink)
- Detects temporary (privacy extension) addresses via `IN6_IFF_CLATED` flag
- Filters link-local, loopback, and ULA addresses by default

## Usage

```toml
[dependencies]
ifaddr6 = "0.1"
```

```rust
use ifaddr6::{get_addresses, select_best};

let addrs = get_addresses("vtnet0").await?;
let best = select_best(&addrs).unwrap();
println!("Best address: {}", best.address);
```

## FreeBSD C Implementation Guide

The FreeBSD path uses C code compiled via the `cc` crate to interact with the kernel. Below is a detailed explanation of the architecture.

### Overview

```text
┌─────────────────────────────────────────────┐
│           ifaddr6 Rust API                   │
│   get_addresses(iface) → Vec<Ipv6AddrInfo>  │
└─────────────────┬───────────────────────────┘
                  │ FFI
┌─────────────────▼───────────────────────────┐
│            freebsd.c (compiled with cc)      │
│                                             │
│  1. getifaddrs()    → enumerate interfaces   │
│  2. ioctl(LIFETIME) → query preferred/valid  │
│  3. ioctl(FLAG)     → query IN6_IFF_CLATED   │
│  4. fill ifaddr6_entry[]                     │
└─────────────────┬───────────────────────────┘
                  │ system calls
┌─────────────────▼───────────────────────────┐
│              FreeBSD Kernel                  │
│  netinet6/in6_var.h: in6_ifreq, ioctl        │
└─────────────────────────────────────────────┘
```

### Step 1: Enumerate Interfaces with `getifaddrs()`

```c
#include <ifaddrs.h>
#include <netinet/in.h>

struct ifaddrs *ifap = NULL;
getifaddrs(&ifap);

for (struct ifaddrs *ifa = ifap; ifa != NULL; ifa = ifa->ifa_next) {
    if (ifa->ifa_addr && ifa->ifa_addr->sa_family == AF_INET6) {
        struct sockaddr_in6 *sin6 = (struct sockaddr_in6 *)ifa->ifa_addr;
        // sin6->sin6_addr contains the IPv6 address
    }
}

freeifaddrs(ifap);
```

**Important:** On FreeBSD, `getifaddrs()` only returns link-local addresses for some interfaces. It is used here primarily for interface enumeration, while the actual lifetime and flag data come from `ioctl()`.

### Step 2: Query Lifetime with `ioctl(SIOCGIFALIFETIME_IN6)`

```c
#include <netinet6/in6_var.h>
#include <sys/ioctl.h>

int s = socket(AF_INET6, SOCK_DGRAM, 0);

struct in6_ifreq ifr6;
memset(&ifr6, 0, sizeof(ifr6));
strncpy(ifr6.ifr_name, "vtnet0", IFNAMSIZ - 1);
ifr6.ifr_addr = *sin6;  /* sockaddr_in6 from getifaddrs */

if (ioctl(s, SIOCGIFALIFETIME_IN6, &ifr6) == 0) {
    struct in6_addrlifetime lt = ifr6.ifr_ifru.ifru_lifetime;
    
    /*
     * in6_addrlifetime structure:
     * - ia6t_expire / ia6t_preferred: absolute expiration (time_t)
     * - ia6t_vltime / ia6t_pltime:   relative seconds (u32)
     *
     * Caveat: On some FreeBSD versions, ia6t_* absolute values are
     * smaller than current epoch time. In that case, use the
     * relative fields (ia6t_pltime / ia6t_vltime).
     */
    if (lt.ia6t_preferred != (time_t)-1 && lt.ia6t_preferred > now)
        pltime = (unsigned int)(lt.ia6t_preferred - now);
    else if (lt.ia6t_pltime != (u_int32_t)-1)
        pltime = lt.ia6t_pltime;
    
    if (lt.ia6t_expire != (time_t)-1 && lt.ia6t_expire > now)
        vltime = (unsigned int)(lt.ia6t_expire - now);
    else if (lt.ia6t_vltime != (u_int32_t)-1)
        vltime = lt.ia6t_vltime;
}
```

### Step 3: Query Flags with `ioctl(SIOCGIFAFLAG_IN6)`

```c
/* Must re-zero ifr6 after previous ioctl! */
memset(&ifr6, 0, sizeof(ifr6));
strncpy(ifr6.ifr_name, "vtnet0", IFNAMSIZ - 1);
ifr6.ifr_addr = *sin6;

if (ioctl(s, SIOCGIFAFLAG_IN6, &ifr6) == 0) {
    u_int32_t flags6 = ifr6.ifr_ifru.ifru_flags6;
    
    /*
     * IN6_IFF_TEMPORARY = 0x0020 (set by kernel for privacy addresses)
     * IN6_IFF_CLATED    = 0x0080 (set for cloned/autoconfigured addresses)
     *
     * Temporary addresses typically have IN6_IFF_CLATED set.
     * Check both flags to be safe.
     */
    is_temporary = flags6 & (IN6_IFF_TEMPORARY | IN6_IFF_CLATED);
}
```

### Complete Data Flow

```
getifaddrs()
    │
    ├─→ AF_INET6 address from ifa_addr
    │
    ├─→ ioctl(SIOCGIFALIFETIME_IN6, &ifr6)
    │       │
    │       ├─→ ifr6.ifr_ifru.ifru_lifetime.ia6t_pltime  → preferred_lft
    │       └─→ ifr6.ifr_ifru.ifru_lifetime.ia6t_vltime  → valid_lft
    │
    └─→ ioctl(SIOCGIFAFLAG_IN6, &ifr6)
            │
            └─→ ifr6.ifr_ifru.ifru_flags6 & IN6_IFF_CLATED → is_temporary
```

### Key Pitfalls

| Problem | Solution |
|---------|----------|
| `ia6t_preferred` < current `time(NULL)` | Fall back to `ia6t_pltime` |
| `IN6_IFF_TEMPORARY` not set on privacy addr | Also check `IN6_IFF_CLATED` (0x80) |
| Second `ioctl` returns wrong flags | Re-zero `in6_ifreq` between ioctls |
| `getifaddrs` missing global addresses on some interfaces | Only use it for enumeration, rely on `ioctl` for data |

### Required Headers

```c
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <net/if.h>
#include <netinet/in.h>
#include <ifaddrs.h>
#include <netinet6/in6_var.h>   /* in6_ifreq, in6_addrlifetime */
```

## License

MIT
