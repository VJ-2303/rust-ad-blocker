# CrabShield Development Guide

## Build, Test, and Run

### Building
```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release
```

### Running
```bash
# Development (using default config.toml)
cargo run

# With specific config
cargo run -- path/to/config.toml

# Release mode (requires sudo for port 53)
sudo ./target/release/CrabShield config.toml

# Testing without sudo (use high port in config)
# Set listen_addr = "0.0.0.0:8053" in config.toml
./target/release/CrabShield config.toml
```

### Code Quality
```bash
# Check code without building
cargo check

# Run linter (clippy)
cargo clippy

# Format code
cargo fmt

# Run all clippy checks
cargo clippy -- -D warnings
```

### Testing
```bash
# Run all tests
cargo test

# Run tests for a specific module
cargo test blocklist

# Run a specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

**Note:** Currently no tests exist in the codebase. When adding tests, use the standard `#[cfg(test)]` module pattern in Rust.

## High-Level Architecture

### DNS Flow (server.rs → dns/handler.rs)
1. **UDP packet arrives** → `server::run()` receives raw bytes on port 53
2. **Extract domain** → `dns/handler.rs` parses DNS wire format, extracts domain name from question section (offset 12), converts to lowercase
3. **Blocklist check** → Query `blocklist.is_blocked(domain_bytes)` using O(1) AHashSet lookup
4. **Fast path (blocked)** → Return NXDOMAIN immediately (< 100μs)
5. **Cache lookup** → Check LRU cache with TTL awareness, patch transaction ID if hit
6. **Upstream forward** → Send to multiplexer with two-socket design for parallel queries
7. **Cache response** → Store upstream result with extracted TTL before returning

### Blocklist Architecture
- **Two-tier storage**: `all_domains` (custom + remote) and `custom_domains` (user additions only)
- **Encoding**: Domains stored as DNS wire format bytes (`\x06google\x03com\x00`) for direct comparison with incoming queries
- **Persistence**: Custom domains saved to file, remote lists fetched every 24 hours from StevenBlack/hosts
- **Update strategy**: Remote updates merge with custom domains, never overwrite user additions
- **Thread safety**: RwLock allows concurrent reads during query processing, brief write locks for modifications

### Cache Strategy (dns/cache.rs)
- **LRU eviction** with 5,000 entry capacity
- **TTL-aware**: Each entry stores `expires_at` timestamp from DNS response
- **Background cleanup**: Every 60 seconds, sweep and remove expired entries
- **Transaction ID patching**: Cached responses reuse bytes but update [0..2] to match current query
- **Thread safety**: Parking_lot Mutex protects LRU map

### Upstream Multiplexer (dns/upstream.rs)
- **Two sockets** bound to random ports prevent head-of-line blocking when one query is slow
- **Transaction ID tracking**: DashMap stores pending requests, keyed by ID, valued by oneshot channel
- **Parallel listening**: Both sockets run `recv_from` loops concurrently
- **Request distribution**: Alternates between sockets for load balancing
- **Timeout**: 5 second timeout per query, then return error

### Admin API (admin/)
- **Axum router** on configurable port (default 9090)
- **Embedded assets**: Static files (HTML/CSS/JS) compiled into binary via `include_dir!` macro
- **API endpoints**:
  - `GET /api/v1/stats` → JSON metrics (queries, blocks, cache, latency)
  - `GET /api/v1/domains/custom` → List user-added domains
  - `POST /api/v1/domains/custom` → Add domain to blocklist + persist
  - `DELETE /api/v1/domains/custom/:domain` → Remove domain + update file
- **State sharing**: `AppState` holds Arc references to `Metrics` and `Blocklist`

### Metrics Collection (metrics.rs)
- **AtomicU64 counters**: Lock-free increment on hot path
- `total_queries`, `blocked_queries`, `cache_hits`, `cache_misses`, `upstream_requests`, `upstream_errors`, `upstream_latency_ms`
- **Start time**: `Instant` captured at creation for uptime calculation
- **Relaxed ordering**: Sufficient for statistics that don't require strict ordering

### Module Organization
```
main.rs          → Entry point, spawns DNS server + background tasks (blocklist fetch, cache cleanup)
server.rs        → UDP receive loop, dispatches to handler
dns/
  handler.rs     → Query processing logic, coordinates blocklist/cache/upstream
  response.rs    → NXDOMAIN construction, TTL extraction from wire format
  cache.rs       → LRU cache with TTL expiration
  upstream.rs    → Two-socket multiplexer for upstream queries
blocklist/
  mod.rs         → Core Blocklist struct with RwLock-protected sets
  encoding.rs    → Convert domains to/from DNS wire format
  persistence.rs → File I/O for custom domains
  loader.rs      → Fetch remote blocklists via reqwest
admin/
  routes.rs      → Axum app definition, handlers, asset serving
  domains.rs     → Domain management endpoint logic
  state.rs       → Shared state for admin API
config.rs        → TOML deserialization
error.rs         → Custom error types (DnsError, LoadError)
metrics.rs       → AtomicU64-based statistics
```

## Key Conventions

### DNS Wire Format Handling
- **Domain encoding**: Use `blocklist::encoding::encode_domain()` to convert strings to DNS wire format (`\x06google\x03com\x00`)
- **Lowercase normalization**: Always `.make_ascii_lowercase()` on extracted domain bytes before lookup
- **Buffer reuse**: Pass `&mut [u8; 255]` stack buffer to avoid allocations in `extract_domain_bytes()`
- **Transaction ID**: First 2 bytes of DNS packet, must be preserved/patched in responses

### Concurrency Patterns
- **RwLock for read-heavy**: `Blocklist.all_domains` uses `parking_lot::RwLock` (multiple readers, single writer)
- **AtomicU64 for metrics**: No locks on query hot path, use `Relaxed` ordering for counters
- **DashMap for pending requests**: Lock-free concurrent map in upstream multiplexer
- **Arc for shared state**: `ServerState` cloned into each task, cheap pointer copy
- **Scoped locks**: Acquire write locks in inner scopes, drop before async I/O (`{ let mut guard = ...; } // dropped`)

### Error Handling
- Custom error types: `DnsError`, `LoadError`
- `Result<T>` alias = `std::result::Result<T, Box<dyn std::error::Error>>`
- `thiserror` for error derive macros
- **Upstream failures**: Return `DnsError::UpstreamTimeout` or `DnsError::UpstreamError`, increment `metrics.upstream_errors`

### File Persistence
- **Async I/O**: Use `tokio::fs` for all file operations
- **Append strategy**: New custom domains appended to file with `append_to_file()`
- **Full rewrite on delete**: `persist_to_file()` writes entire custom domain set (to remove deleted entry)
- **Path handling**: `blocklist_path` in config is relative to config file location

### Logging
- **Tracing**: Use `tracing::info!`, `warn!`, `error!` macros
- **Structured fields**: `info!(domain_count = count, "message")` format
- **Log level**: Set via `log_level` in config.toml (error/warn/info/debug/trace)
- **Initialization**: `tracing_subscriber::fmt()` with `EnvFilter` in main.rs

### Background Tasks
- **Blocklist fetch**: Runs every 24 hours, 3 retry attempts with 10s backoff
- **Cache cleanup**: Every 60 seconds, remove expired entries
- **Task isolation**: Each spawned with `tokio::spawn()`, errors logged but don't crash main server

### Configuration
- **TOML format**: `serde` + `toml` crate for parsing
- **Required fields**: `listen_addr`, `upstream_dns`, `blocklist_path`, `log_level`, `admin_addr`
- **Address format**: "IP:PORT" strings, parsed to `SocketAddr`
- **Default path**: `config.toml` in current directory if not specified

### Binary Optimization
- **Release profile** in Cargo.toml:
  - `opt-level = 3` (max optimization)
  - `lto = "fat"` (link-time optimization)
  - `codegen-units = 1` (better optimization, slower compile)
  - `strip = true` (remove debug symbols)
  - `panic = "abort"` (smaller binary, no unwinding)

### Asset Embedding
- **Compile-time inclusion**: `include_dir!("$CARGO_MANIFEST_DIR/static/admin")` in routes.rs
- **No runtime file I/O**: All HTML/CSS/JS bundled into binary
- **MIME type detection**: `mime_guess` crate infers content-type from file extension
- **Path handling**: Strip leading `/` from request paths before lookup

### Naming Conventions
- **Modules**: snake_case, single file or directory with mod.rs
- **Types**: PascalCase (Blocklist, ServerState)
- **Functions**: snake_case (handle_query, extract_domain_bytes)
- **Constants**: SCREAMING_SNAKE_CASE (ADMIN_ASSETS)
- **Async functions**: Prefix with `async fn`, always return `Result` when fallible

## Development Tips

### Port 53 Requires Root
- **Development**: Use high port (8053) in config, test without sudo
- **Production**: Port 53 needs `sudo` or `CAP_NET_BIND_SERVICE` capability
- **Alternative**: Use `setcap cap_net_bind_service=+ep target/release/CrabShield` to run without sudo

### Testing DNS Server
```bash
# Test with dig
dig @127.0.0.1 -p 53 google.com

# Test blocked domain (should return NXDOMAIN)
dig @127.0.0.1 -p 53 ads.example.com

# Check admin API
curl http://localhost:9090/api/v1/stats | jq
```

### Adding New Dependencies
- Keep binary size minimal
- Prefer `default-features = false` when possible (see reqwest with rustls)
- Use `parking_lot` instead of `std::sync` for better RwLock/Mutex performance
- Use `ahash` for domain hash maps (faster than default hasher for short strings)

### Blocklist Format
- **One domain per line**: `example.com`
- **No protocols**: Don't include `http://` or `https://`
- **Subdomains supported**: `ads.example.com` blocks exactly that subdomain
- **Hosts file parsing**: `loader.rs` strips `127.0.0.1` prefixes and comments from StevenBlack format
