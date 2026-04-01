# CrabShield

A high-performance DNS-based ad blocker built in Rust that intercepts unwanted domains at the network level. Rather than blocking ads in the browser, CrabShield operates as a DNS server that returns NXDOMAIN responses for blocked domains, effectively preventing your devices from ever connecting to ad servers, trackers, and malicious sites.

## What It Does

CrabShield sits between your devices and the internet, acting as a custom DNS resolver. When any device on your network tries to reach a domain, CrabShield checks its blocklist first. Clean domains get forwarded to your chosen upstream DNS provider (Google, Cloudflare, etc.), while blocked domains receive an immediate NXDOMAIN response—telling the requesting application that the domain doesn't exist.

This approach works across all applications and devices on your network, without requiring browser extensions or per-device configuration. Block ads on smart TVs, mobile apps, IoT devices, and any other network-connected hardware that respects DNS responses.

## Core Features

**DNS-Level Blocking**  
Intercepts domain resolution requests before they reach the internet. Blocked domains never establish a connection, saving bandwidth and improving privacy.

**Intelligent Caching**  
LRU-based cache with TTL respect reduces upstream DNS queries by storing recent lookups. The cache automatically expires entries and cleans stale data, keeping memory usage bounded while maximizing hit rates.

**Dynamic Blocklist Updates**  
Automatically fetches updated blocklists from the internet every 24 hours. The system attempts three retries with backoff if fetching fails, ensuring your protection stays current without manual intervention.

**Custom Domain Management**  
Add or remove domains through the admin API. Custom additions persist to disk and survive restarts. The blocklist separates user-defined domains from fetched lists, allowing you to merge both sources seamlessly.

**Web Dashboard**  
Embedded admin interface served on a configurable port. View statistics, monitor performance, manage custom blocked domains—all through a clean web UI bundled into the binary at compile time.

**Concurrent Safety**  
RwLock-protected blocklist and LRU cache allow multiple readers simultaneously. Writes lock briefly, and atomic counters track metrics without contention. The entire server is built on Tokio's async runtime for efficient concurrency.

---

## Setup Instructions

### Linux 

```bash
curl -sSL https://raw.githubusercontent.com/VJ-2303/CrabShield/main/install.sh | sudo bash
```

### Windows

```powershell
irm https://raw.githubusercontent.com/VJ-2303/CrabShield/main/install.ps1 | iex
```

### Verify Installation

```bash
# Linux
CrabShield --version

# Windows (PowerShell)
CrabShield.exe --version
```

Key settings in `config.toml`:

```toml
listen_addr = "0.0.0.0:53"          # DNS server address
upstream_dns = "8.8.8.8:53"         # Upstream resolver
blocklist_path = "blocklists/default.txt"
log_level = "info"
admin_addr = "0.0.0.0:9090"         # Web dashboard
```

**Running the Server**

On Linux (requires root for port 53):
```bash
CrabShield path_to_the_config_file
```

On Windows:
```powershell
CrabShield.exe path_to_the_config_file
```

**Configure Your Devices**

Point your device DNS settings to the machine running CrabShield. You can set this per-device or configure your router's DHCP to advertise CrabShield as the network DNS server.

**Access the Dashboard**

Open `http://<server-ip>:9090/admin` in your browser to view statistics and manage custom domains.

![Admin Web](https://raw.githubusercontent.com/VJ-2303/CrabShield/refs/heads/main/demo/admin_page.png)

---

## Technical Architecture

**DNS Protocol Handling**  
Parses DNS queries at the byte level without heavyweight libraries. Extracts domain names from the question section, maintains transaction IDs, and constructs NXDOMAIN responses manually. This low-level approach eliminates dependencies and keeps the binary small.

**Blocklist Storage**  
Domains encode as lowercase byte sequences and live in an AHashSet for O(1) lookups. The fast hash function optimizes for short domain strings, and RwLock enables concurrent reads during query processing.

**Cache Design**  
LRU eviction policy keeps the hottest domains in memory. Each cache entry stores the full DNS response bytes and an expiration timestamp. A background task periodically sweeps expired entries, while the LRU naturally pushes out cold data when capacity is reached.

**Upstream Strategy**  
Two separate UDP sockets bind to random ports for upstream communication. A DashMap tracks pending requests by transaction ID. When a response arrives on either socket, the multiplexer matches the ID and delivers the response through a oneshot channel. This design prevents head-of-line blocking when upstream resolvers have variable latency.

**Web Stack**  
Axum handles HTTP routing with minimal overhead. Static assets embed at compile time via include_dir, eliminating runtime file I/O. JSON endpoints return metrics and handle domain management operations.

---

## Technology Stack

**Language & Runtime**  
Rust with Tokio async runtime provides memory safety, zero-cost abstractions, and efficient concurrency without garbage collection pauses.

**Core Dependencies**  
- **tokio**: Async I/O, UDP/TCP sockets, task scheduling
- **bytes**: Zero-copy buffer operations for DNS packets
- **parking_lot**: Fast RwLock and Mutex implementations
- **ahash**: Optimized hash function for domain lookups
- **dashmap**: Lock-free concurrent hashmap for pending requests
- **lru**: Least-recently-used cache eviction policy

**HTTP & Serialization**  
- **axum**: Fast, ergonomic web framework
- **serde**: Type-safe JSON serialization
- **include_dir**: Compile-time asset embedding

**Configuration & Logging**  
- **toml**: Human-readable config file parsing
- **tracing**: Structured logging with dynamic levels

---

## Performance Characteristics

**Query Latency**  
Blocked domains return in under 100 microseconds—just the time to extract the domain, check the hashset, and write the NXDOMAIN response. Cached queries take slightly longer to retrieve and patch the transaction ID but still finish in microseconds. Cache misses require an upstream round-trip, typically 10-50ms depending on your DNS provider.

**Memory Usage**  
A blocklist with 100,000 domains consumes roughly 15-20MB. The LRU cache caps at 5,000 entries, adding another 5-10MB depending on response sizes. Total resident memory typically stays under 50MB, making this suitable for embedded devices like Raspberry Pi.

**Throughput**  
The async architecture handles thousands of concurrent queries without spawning threads per request. Atomic metrics and lock-free pending request tracking keep contention minimal. On a typical home network, a single instance easily saturates gigabit bandwidth before becoming CPU-bound.

---

## Future Improvements

**Advanced Filtering**  
Regular expression support for wildcard blocking. Block entire TLDs or pattern-match subdomains without listing every variant explicitly.

**Allowlist Support**  
Explicit allowlist to override blocklist entries. Useful when legitimate domains accidentally appear on public blocklists.

**Sharded Cache**  
Partition the LRU cache into multiple shards based on domain hash. Reduces lock contention on high-traffic deployments by allowing parallel cache operations.

**Multiple Blocklist Sources**  
Configure multiple blocklist URLs with different update intervals. Merge lists from different providers and allow per-list enable/disable.

---

## Contributing

This project is a learning exercise in high-performance systems programming with Rust. Contributions, bug reports, and suggestions are welcome. The codebase prioritizes clarity and simplicity—patches should maintain these qualities.
