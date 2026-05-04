# ifaddr6

Cross-platform IPv6 address discovery with lifetime information.

## Features

- **FreeBSD**: `getifaddrs()` + `ioctl(SIOCGIFALIFETIME_IN6)`
- **Linux**: (coming soon — rtnetlink)
- Detects temporary (privacy extension) addresses via RFC 4941 heuristic
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

## License

MIT
