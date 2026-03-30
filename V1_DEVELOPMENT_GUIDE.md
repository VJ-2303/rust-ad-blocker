# AdBlocker — v1 Development Guide
## Bugs, Fixes, Optimizations & Missing Features

This document covers every known bug, performance issue, and missing feature in the
current codebase. Each item identifies the exact file(s) to change, shows the
problematic code, provides the replacement, and explains why the fix is correct.
Work through sections in order — critical bugs first, then performance, then features.
All changes are self-contained and modular; each can be applied independently.

---

## Quick Reference Table

| ID  | Category         | Severity   | File(s)                                                   | Summary                                                              |
|-----|------------------|------------|-----------------------------------------------------------|----------------------------------------------------------------------|
| B-1 | Critical Bug     | 🔴 High    | `src/blocklist/mod.rs`                                    | `all_domains` is never populated at startup — nothing is ever blocked |
| B-2 | Critical Bug     | 🔴 High    | `src/blocklist/mod.rs`                                    | `add_custom_domain()` skips `all_domains` — new blocks never take effect |
| B-3 | Critical Bug     | 🔴 High    | `src/server.rs`                                           | `.unwrap()` on socket receive kills the entire DNS server on any OS error |
| B-4 | Bug              | 🟠 Medium  | `src/blocklist/mod.rs`, `loader.rs`, `src/dns/handler.rs` | Blocklist matching is case-sensitive — uppercase queries bypass blocks |
| B-5 | Bug              | 🟠 Medium  | `src/blocklist/mod.rs`                                    | `len()` always returns `0` at startup — misleading log output         |
| B-6 | Bug              | 🟡 Low     | `src/dns/upstream.rs`, `src/error.rs`                     | Wrong error variant used when the upstream response channel drops     |
| B-7 | Bug              | 🟡 Low     | `src/main.rs`, `src/config.rs`, `config.toml`             | Admin port (`8080`) and config file path are hardcoded in source      |
| B-8 | Bug              | 🟡 Low     | `src/main.rs`                                             | Startup log contains typo: `"RustHoldatae"`                           |
| P-1 | Performance      | 🟠 Medium  | `src/blocklist/mod.rs`, `loader.rs`, `Cargo.toml`         | `HashSet` uses slow SipHash — `AHashSet` gives 2–4× faster lookups   |
| P-2 | Performance      | 🟠 Medium  | `src/blocklist/loader.rs`                                 | HTTP fetch has no timeout — a slow server hangs the update loop forever |
| P-3 | Performance      | 🟡 Low     | `src/dns/cache.rs`, `src/main.rs`                         | `clean_expired()` holds the mutex while scanning all 10 000 entries   |
| P-4 | Performance      | 🟡 Low     | `src/blocklist/mod.rs`                                    | `update_list()` holds both locks longer than necessary                |
| F-1 | Missing Feature  | 🟠 Medium  | `src/blocklist/mod.rs`, `src/admin/domains.rs`, `routes.rs` | No `DELETE` endpoint — custom domains cannot be removed via API     |
| F-2 | Missing Feature  | 🟠 Medium  | `src/main.rs`                                             | No graceful shutdown — process must be force-killed                   |
| F-3 | Missing Feature  | 🟡 Low     | `src/config.rs`, `src/error.rs`                           | Config values are never validated — bad addresses panic deep in startup |
| C-1 | Code Quality     | ⚪ Low     | `src/dns/handler.rs`, `src/dns/packet.rs`                 | Commented-out logging code should be restored — it is useful and correct |

---

## Section 1 — Critical Bugs

These three bugs directly break core functionality. The DNS blocker does not work
correctly until all three are resolved.

---

### B-1 — `all_domains` Is Never Populated at Startup

**File:** `src/blocklist/mod.rs`

#### The Problem

`Blocklist::load()` reads the local file and stores every domain in
`self.custom_domains`. However, `is_blocked()` only ever reads `self.all_domains`.

At startup, `all_domains` is an empty `HashSet`. It is only populated when
`update_list()` is called — which only happens after the background HTTP fetch
succeeds. That fetch can take several seconds, and if all three retry attempts fail
(e.g. the machine is offline), `all_domains` remains empty for the entire 24-hour
cycle.

The broken execution path is:
```
load()       → stores domains in custom_domains   ✓
is_blocked() → reads all_domains                  ← EMPTY → returns false → NOTHING BLOCKED
```

#### The Fix

In `Blocklist::load()`, initialise `all_domains` with the same set of domains that
was just loaded from the file. This gives `is_blocked()` correct data immediately.
`update_list()` continues to work exactly as designed — it overwrites `all_domains`
with `remote ∪ custom` whenever the remote list arrives.

**In `src/blocklist/mod.rs`, find the return statement at the end of `load()` and
change it:**

FROM:
```rust
Ok(Self {
    custom_domains: RwLock::new(domains),
    all_domains: RwLock::new(HashSet::new()),
    custom_path: path.to_string(),
})
```

TO:
```rust
let initial_all = domains.clone();
Ok(Self {
    custom_domains: RwLock::new(domains),
    all_domains: RwLock::new(initial_all),
    custom_path: path.to_string(),
})
```

#### Why This Works

Both sets now start with the local file's content. `is_blocked()` returns a correct
answer from the first query. When the remote fetch later completes and `update_list()`
is called, it replaces `all_domains` with the larger combined set — the fix does not
interfere with that flow at all.

---

### B-2 — `add_custom_domain()` Does Not Update `all_domains`

**File:** `src/blocklist/mod.rs`

#### The Problem

When a user calls `POST /api/v1/domains/custom`, the domain is added to
`self.custom_domains` and written to disk. `all_domains` is never touched. Since
`is_blocked()` only reads `all_domains`, the newly added domain is silently ignored
until the next `update_list()` call — 24 hours later.

#### The Fix

After inserting into `custom_domains`, also insert the same encoded bytes into
`all_domains`. The two write locks must be acquired and released in sequence, never
simultaneously, to prevent a deadlock.

**In `src/blocklist/mod.rs`, find the locking block inside `add_custom_domain()` and
change it:**

FROM:
```rust
{
    let mut guard = self.custom_domains.write();
    if !guard.insert(encoded) {
        return Ok(());
    }
}
```

TO:
```rust
{
    let mut custom_guard = self.custom_domains.write();
    if !custom_guard.insert(encoded.clone()) {
        return Ok(());
    }
}
// Acquire all_domains separately — never hold two write locks at the same time
{
    let mut all_guard = self.all_domains.write();
    all_guard.insert(encoded);
}
```

#### Why This Works

`all_domains` is updated immediately inside the same function call. The very next DNS
query after the API returns will correctly block the new domain. The sequential lock
acquisition pattern prevents deadlocks that would occur if both locks were held
simultaneously.

---

### B-3 — `.unwrap()` on Socket Receive Panics the DNS Server

**File:** `src/server.rs`

#### The Problem

The main receive loop unconditionally unwraps the result of `recv_buf_from`:

```rust
let (len, addr) = state.socket.recv_buf_from(&mut buf).await.unwrap();
```

If the operating system returns any socket error (e.g. a transient buffer overflow,
an `ICMP Port Unreachable` response that surfaces as an error on some platforms, or a
permission change), `.unwrap()` panics. Because the DNS server runs inside a
`tokio::spawn`ed task, the panic causes that task to exit silently. The admin API
keeps running — metrics show the server is "up" — but no DNS queries are answered.
This is the worst kind of failure: invisible to basic monitoring.

#### The Fix

Replace `.unwrap()` with a `match` that logs the error and continues the loop. A
transient OS error on a UDP socket is never a reason to stop serving.

**In `src/server.rs`, find the receive line and replace it:**

FROM:
```rust
let (len, addr) = state.socket.recv_buf_from(&mut buf).await.unwrap();
```

TO:
```rust
let (len, addr) = match state.socket.recv_buf_from(&mut buf).await {
    Ok(result) => result,
    Err(e) => {
        error!(error = %e, "Socket receive error — skipping packet");
        continue;
    }
};
```

#### Why This Works

Transient errors are logged and skipped; the loop immediately waits for the next
packet. If a real, persistent error occurs, it will be logged on every iteration,
making it immediately visible in the logs without crashing the process.

---

## Section 2 — Important Bugs

These bugs cause incorrect, misleading, or unintended behaviour but do not completely
prevent the server from running.

---

### B-4 — Domain Matching Is Case-Sensitive

**Files:** `src/blocklist/mod.rs`, `src/blocklist/loader.rs`, `src/dns/handler.rs`

#### The Problem

DNS domain names are case-insensitive by specification (RFC 1034 §3.1). The blocklist
stores domains in DNS wire format as raw bytes and uses a direct byte-for-byte
comparison. If a query arrives for `ADS.EXAMPLE.COM` but the blocklist entry is
`ads.example.com`, the bytes differ and `is_blocked()` returns `false` — the block
is bypassed.

There are three places where normalisation must happen:

1. `src/blocklist/mod.rs` `load()` — reads the local file without lowercasing.
2. `src/blocklist/loader.rs` `fetch_remote_blocklist()` — parses remote hosts without lowercasing.
3. `src/dns/handler.rs` `handle_query()` — extracts raw wire-format bytes from the
   DNS packet with no normalisation before the blocklist lookup.

#### The Fixes

**Fix 1 — `src/blocklist/mod.rs` inside `load()`:**

FROM:
```rust
domains.insert(encode_domain(line));
```

TO:
```rust
domains.insert(encode_domain(&line.to_lowercase()));
```

**Fix 2 — `src/blocklist/loader.rs` inside `fetch_remote_blocklist()`:**

FROM:
```rust
blocklist.insert(encode_domain(domain));
```

TO:
```rust
blocklist.insert(encode_domain(&domain.to_lowercase()));
```

**Fix 3 — `src/dns/handler.rs` inside `handle_query()`:**

After the byte-loop that extracts the domain, create an owned, lowercased copy before
any comparison or cache operation. DNS wire format labels consist entirely of ASCII
bytes, so `make_ascii_lowercase()` is both correct and allocation-free on the
existing buffer.

FROM:
```rust
let domain_bytes = &packet_bytes[12..=i];

if state.blocklist.is_blocked(domain_bytes) {
```

TO:
```rust
let mut domain_bytes = packet_bytes[12..=i].to_vec();
// Normalise to lowercase — DNS names are case-insensitive (RFC 1034 §3.1)
for byte in domain_bytes.iter_mut() {
    byte.make_ascii_lowercase();
}
let domain_bytes = domain_bytes; // rebind as immutable

if state.blocklist.is_blocked(&domain_bytes) {
```

Also update the two places below it that reference `domain_bytes` for the cache path:

FROM:
```rust
if let Some(cached_bytes) = state.cache.get(domain_bytes, &packet_bytes[0..2]) {
    ...
}
let domain_owned = domain_bytes.to_vec();
```

TO:
```rust
if let Some(cached_bytes) = state.cache.get(&domain_bytes, &packet_bytes[0..2]) {
    ...
}
let domain_owned = domain_bytes; // already an owned Vec<u8> after the fix above
```

#### Why This Works

All domain names — from the local file, the remote list, and incoming DNS queries —
are normalised to ASCII lowercase before any comparison. A query for
`DOUBLECLICK.NET` will now correctly match `doubleclick.net` in the blocklist.

---

### B-5 — `len()` Always Returns `0` at Startup

**File:** `src/blocklist/mod.rs`

#### The Problem

`len()` reads `all_domains.len()`, which (before B-1 is fixed) starts at zero:

```rust
pub fn len(&self) -> usize {
    self.all_domains.read().len()
}
```

The startup log therefore always prints `domain_count = 0`, giving the false
impression that the blocklist file was empty or did not load.

#### The Fix

This is automatically resolved when B-1 is applied, since `all_domains` is now
initialised from the file. However, also add an `is_empty()` companion method — Rust
Clippy produces a `len_without_is_empty` warning for any type that implements `len()`
without `is_empty()`.

**In `src/blocklist/mod.rs`, add this method directly after `len()`:**

```rust
pub fn is_empty(&self) -> bool {
    self.all_domains.read().is_empty()
}
```

No change to `len()` itself is required once B-1 is applied.

---

### B-6 — Wrong Error Variant on Upstream Channel Drop

**Files:** `src/dns/upstream.rs`, `src/error.rs`

#### The Problem

In `upstream.rs`, when the `oneshot` receiver gets a `RecvError` (meaning the sender
half was dropped before sending a response), the error returned is
`DnsError::NoQueries`:

```rust
Ok(Err(_)) => Err(AppError::Dns(crate::error::DnsError::NoQueries)),
```

`NoQueries` means "the DNS packet contained no query records" — a completely unrelated
condition. When this error surfaces in logs during an upstream instability event, it
produces a misleading message that directs debugging in the wrong direction.

#### The Fix

**Step 1 — Add a new variant to the `DnsError` enum in `src/error.rs`:**

Inside the `DnsError` enum, add:
```rust
#[error("Upstream response channel closed unexpectedly")]
UpstreamChannelClosed,
```

**Step 2 — Use the new variant in `src/dns/upstream.rs`:**

FROM:
```rust
Ok(Err(_)) => Err(AppError::Dns(crate::error::DnsError::NoQueries)),
```

TO:
```rust
Ok(Err(_)) => Err(AppError::Dns(crate::error::DnsError::UpstreamChannelClosed)),
```

---

### B-7 — Admin Port and Config Path Are Hardcoded

**Files:** `src/main.rs`, `src/config.rs`, `config.toml`

#### The Problem

Two values that should be configurable are hardcoded in source:

1. Admin API port: `tokio::net::TcpListener::bind("0.0.0.0:8080")`
2. Config file path: `config::Config::load("config.toml")`

A user cannot change either without recompiling the binary.

#### The Fix — Config file path

Support passing the path as the first command-line argument, falling back to
`"config.toml"` when none is given.

**In `src/main.rs`, change the config load line:**

FROM:
```rust
let config = config::Config::load("config.toml")?;
```

TO:
```rust
let config_path = std::env::args()
    .nth(1)
    .unwrap_or_else(|| "config.toml".to_string());
let config = config::Config::load(&config_path)?;
```

The user can now run `./AdBloacker /etc/adblocker/config.toml`.

#### The Fix — Admin address

**Step 1 — Add `admin_addr` to the `Config` struct in `src/config.rs`:**

```rust
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_dns: String,
    pub blocklist_path: String,
    pub log_level: String,
    #[serde(default = "Config::default_admin_addr")]
    pub admin_addr: String,
}

impl Config {
    fn default_admin_addr() -> String {
        "0.0.0.0:8080".to_string()
    }

    pub fn load(path: &str) -> Result<Self, crate::error::ConfigError> {
        let config_str = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&config_str)?;
        Ok(config)
    }
}
```

The `#[serde(default = ...)]` attribute means existing `config.toml` files without
the new field continue to work — they silently get the default of `"0.0.0.0:8080"`.

**Step 2 — Add the field to `config.toml`:**

```toml
listen_addr   = "0.0.0.0:8053"
upstream_dns  = "1.1.1.1:53"
blocklist_path = "blocklists/default.txt"
log_level     = "info"
admin_addr    = "0.0.0.0:8080"
```

**Step 3 — Use `config.admin_addr` in `src/main.rs`:**

FROM:
```rust
tracing::info!("Starting Admin Web API on 0.0.0.0:8080");
let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
```

TO:
```rust
tracing::info!(admin_addr = %config.admin_addr, "Starting Admin Web API");
let listener = tokio::net::TcpListener::bind(&config.admin_addr).await?;
```

---

### B-8 — Startup Log Typo

**File:** `src/main.rs`

#### The Problem

The startup log message reads `"Starting RustHoldatae DNS Server"`. The word
`"RustHoldatae"` is a nonsense string — likely a leftover from a draft.

#### The Fix

FROM:
```rust
info!(
    listen_addr = %config.listen_addr,
    upstream = %config.upstream_dns,
    "Starting RustHoldatae DNS Server"
);
```

TO:
```rust
info!(
    listen_addr = %config.listen_addr,
    upstream = %config.upstream_dns,
    "Starting AdBlocker DNS Server"
);
```

---

## Section 3 — Performance Optimizations

---

### P-1 — Replace `HashSet` with `AHashSet` for Blocklist Lookups

**Files:** `src/blocklist/mod.rs`, `src/blocklist/loader.rs`, `Cargo.toml`

#### The Problem

`std::collections::HashSet` uses SipHash as its default hash function. SipHash is
designed to resist HashDoS attacks, which makes it appropriate for web server session
maps and similar structures exposed to attacker-controlled keys. For a local DNS
blocker, this protection is unnecessary and its cost is not.

Every DNS query hits `is_blocked()`, which hashes the domain name bytes. Under load
(thousands of queries per second), the accumulated hashing overhead is measurable.
The `ahash` crate's AHash algorithm is 2–4× faster than SipHash on short byte-slice
keys with no meaningful downside for this use case.

#### The Fix

**Step 1 — Add `ahash` to `Cargo.toml`:**

In the `[dependencies]` section, add:
```toml
ahash = "0.8"
```

**Step 2 — Update `src/blocklist/mod.rs`:**

Replace the import:
```rust
use std::collections::HashSet;
```

With:
```rust
use ahash::AHashSet;
```

Replace all `HashSet<Vec<u8>>` type annotations in the struct definition:

```rust
pub struct Blocklist {
    pub all_domains: RwLock<AHashSet<Vec<u8>>>,
    pub custom_domains: RwLock<AHashSet<Vec<u8>>>,
    pub custom_path: String,
}
```

In `load()`, replace:
```rust
let mut domains = HashSet::new();
```

With:
```rust
let mut domains = AHashSet::new();
```

In `update_list()`, update the parameter type:
```rust
pub fn update_list(&self, remote: AHashSet<Vec<u8>>) {
```

**Step 3 — Update `src/blocklist/loader.rs`:**

Replace:
```rust
use std::{collections::HashSet, u8};
```

With:
```rust
use ahash::AHashSet;
```

Update the function signature and the internal declaration:
```rust
pub async fn fetch_remote_blocklist(url: &str) -> Result<AHashSet<Vec<u8>>, AppError> {
    ...
    let mut blocklist: AHashSet<Vec<u8>> = AHashSet::new();
```

#### Why This Works

AHash produces identical correctness guarantees for equality-based lookups while
being significantly faster. The blocklist lookup is on the hot path for every single
DNS query, so this improvement compounds under load. Swapping the hash function is a
zero-risk change — the data structure behaviour is identical, only the internal hash
computation changes.

---

### P-2 — Add a Timeout to the Remote Blocklist HTTP Fetch

**File:** `src/blocklist/loader.rs`

#### The Problem

`reqwest::get(url)` uses a `Client` with no configured timeout. If the remote CDN is
slow to respond or the connection stalls, `fetch_remote_blocklist()` awaits
indefinitely. The `tokio::spawn` task holding this future is parked forever, consuming
memory and preventing the next update cycle from starting.

Additionally, `reqwest::get()` is a convenience function that allocates a new internal
`Client` on every call. For a 24-hour background task this overhead is negligible in
isolation, but coupling it with an explicit `Client::builder()` call is the correct
pattern and naturally opens the door for the timeout configuration.

#### The Fix

**In `src/blocklist/loader.rs`, replace the `reqwest::get()` call with an explicit
client that has a 30-second timeout:**

FROM:
```rust
pub async fn fetch_remote_blocklist(url: &str) -> Result<HashSet<Vec<u8>>, AppError> {
    let text = reqwest::get(url).await?.text().await?;
```

TO:
```rust
pub async fn fetch_remote_blocklist(url: &str) -> Result<AHashSet<Vec<u8>>, AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("AdBlocker/1.0")
        .build()?;

    let text = client.get(url).send().await?.text().await?;
```

The `reqwest::Client::builder().build()?` call returns a `reqwest::Error` on failure,
which is already covered by the existing `AppError::Network(#[from] reqwest::Error)`
variant — no error handling changes are needed.

#### Why This Works

If the remote server does not respond within 30 seconds, `reqwest` returns an error.
The retry loop in `main.rs` already handles this: it logs the failure, waits 10
seconds, and retries up to 3 times. The overall retry behaviour is unchanged; the
code simply can no longer hang indefinitely.

---

### P-3 — Limit Mutex Hold Time in `clean_expired()`

**Files:** `src/dns/cache.rs`, `src/main.rs`

#### The Problem

`clean_expired()` acquires the `parking_lot::Mutex`, iterates over all entries in the
`LruCache` (up to 10 000), collects expired keys into a `Vec`, then removes them —
all while holding the mutex. Every concurrent DNS query that needs the cache (i.e.,
every query) is blocked waiting for this mutex during the entire scan.

The cleanup runs every 300 seconds. While 300-second intervals are infrequent, the
brief periodic latency spike under load is unnecessary, because the cache already does
lazy expiry in `get()` — expired entries are removed when they are next accessed.
`clean_expired()` is a safety net for entries that are never re-requested, not a
primary expiry mechanism.

#### The Fix

Cap the number of entries removed per cleanup pass at a small fixed number (200).
Combined with reducing the cleanup interval (so that the capped sweeps collectively
cover the same volume), this keeps any single mutex hold short and predictable.

**In `src/dns/cache.rs`, replace the `clean_expired()` body:**

FROM:
```rust
pub fn clean_expired(&self) {
    let now = Instant::now();

    let mut cache = self.store.lock();

    let expired_key: Vec<Vec<u8>> = cache
        .iter()
        .filter(|(_, entry)| entry.expires_at <= now)
        .map(|(key, _)| key.clone())
        .collect();

    for key in expired_key {
        cache.pop(&key);
    }
}
```

TO:
```rust
pub fn clean_expired(&self) {
    const MAX_EVICTIONS_PER_CYCLE: usize = 200;
    let now = Instant::now();
    let mut cache = self.store.lock();

    let expired_keys: Vec<Vec<u8>> = cache
        .iter()
        .filter(|(_, entry)| entry.expires_at <= now)
        .map(|(key, _)| key.clone())
        .take(MAX_EVICTIONS_PER_CYCLE)
        .collect();

    for key in expired_keys {
        cache.pop(&key);
    }
}
```

**In `src/main.rs`, reduce the cleanup interval from 300 seconds to 60 seconds:**

FROM:
```rust
tokio::time::sleep(Duration::from_secs(300)).await;
```

TO:
```rust
tokio::time::sleep(Duration::from_secs(60)).await;
```

#### Why This Works

Each cycle now processes at most 200 entries before releasing the lock. The
combination of smaller batches and more frequent runs provides equivalent stale-entry
eviction throughput. A single cleanup pass now holds the mutex for microseconds rather
than milliseconds, eliminating the periodic latency spike entirely.

---

### P-4 — Minimise Lock Scope in `update_list()`

**File:** `src/blocklist/mod.rs`

#### The Problem

The current implementation holds a read lock on `custom_domains` for the entire
duration of both the clone and the merge:

```rust
pub fn update_list(&self, remote: HashSet<Vec<u8>>) {
    let custom = self.custom_domains.read().clone(); // read lock held...
    let mut all = remote;
    all.extend(custom);
    *self.all_domains.write() = all; // ...until here
}
```

While the merge is being built (potentially many thousands of entries), any concurrent
call to `add_custom_domain()` that needs a write lock on `custom_domains` is blocked.
This is unnecessary: the only operation that actually requires the lock is the
`.clone()` call.

#### The Fix

Use a block `{ }` to force the `RwLockReadGuard` to drop as soon as the clone is
complete. The merge is then built without holding any lock, and the write lock on
`all_domains` is only acquired for the final swap — which is a single pointer move and
extremely fast.

**In `src/blocklist/mod.rs`, replace `update_list()`:**

FROM:
```rust
pub fn update_list(&self, remote: HashSet<Vec<u8>>) {
    let custom = self.custom_domains.read().clone();
    let mut all = remote;
    all.extend(custom);
    *self.all_domains.write() = all;
}
```

TO:
```rust
pub fn update_list(&self, remote: AHashSet<Vec<u8>>) {
    // Clone while holding the read lock, then drop it immediately
    let custom_snapshot = {
        let guard = self.custom_domains.read();
        guard.clone()
    }; // <-- read lock on custom_domains is released here

    // Build the merged set with no locks held
    let mut all = remote;
    all.extend(custom_snapshot);

    // Acquire the write lock only for the final swap (a single move)
    *self.all_domains.write() = all;
}
```

#### Why This Works

The read lock on `custom_domains` is now held for the minimum possible time (just the
clone). Any `add_custom_domain()` call that arrives during a remote list update can
acquire its write lock as soon as the snapshot is taken. The write lock on
`all_domains` is held for a nanosecond-scale move operation — imperceptible to
concurrent readers.

---

## Section 4 — Missing v1 Features

---

### F-1 — Add a `DELETE` Endpoint for Custom Domains

**Files:** `src/blocklist/mod.rs`, `src/admin/domains.rs`, `src/admin/routes.rs`

#### The Problem

Users can add custom blocked domains via `POST /api/v1/domains/custom` but there is
no way to remove them through the API. The only way to unblock a domain is to manually
edit the file on disk and restart the server. This is a missing half of the CRUD
surface and makes the API incomplete for v1.

#### Implementation

This change requires edits in three files.

---

**Step 1 — Add `remove_custom_domain()` and `persist_custom_domains()` to
`src/blocklist/mod.rs`**

Add the following two methods to the `Blocklist` impl block, after
`add_custom_domain`:

```rust
pub async fn remove_custom_domain(&self, domain: &str) -> std::io::Result<bool> {
    let domain = domain.trim().to_lowercase();
    let encoded = encode_domain(&domain);

    // Remove from custom_domains first
    let was_removed = {
        let mut custom_guard = self.custom_domains.write();
        custom_guard.remove(&encoded)
    };

    if !was_removed {
        // Domain was not in the custom list; nothing to do
        return Ok(false);
    }

    // Also evict from all_domains so blocking stops on the next query
    {
        let mut all_guard = self.all_domains.write();
        all_guard.remove(&encoded);
    }

    // Rewrite the on-disk file to reflect the removal
    self.persist_custom_domains().await?;
    Ok(true)
}

/// Rewrites the entire custom blocklist file from the current in-memory set.
/// Called after every removal to keep disk state consistent with memory.
async fn persist_custom_domains(&self) -> std::io::Result<()> {
    let content = {
        let guard = self.custom_domains.read();
        let mut sorted: Vec<String> = guard.iter().map(|b| decode_domain(b)).collect();
        sorted.sort();
        let mut lines = String::from("# Custom blocked domains\n");
        for domain in sorted {
            lines.push_str(&domain);
            lines.push('\n');
        }
        lines
    };
    tokio::fs::write(&self.custom_path, content).await
}
```

Note: The `add_custom_domain()` method appends to the file; the new
`persist_custom_domains()` rewrites it entirely. This is intentional — appending is
efficient for adding, but removal has no clean append representation. Rewriting the
full file on removal is correct since removals are expected to be infrequent.

---

**Step 2 — Add the `remove_custom_domain` handler to `src/admin/domains.rs`**

Add the following function. It reuses the existing `AddDomainRequest` struct since
the request body shape is identical (`{"domain": "..."}`):

```rust
pub async fn remove_custom_domain(
    State(state): State<AppState>,
    Json(payload): Json<AddDomainRequest>,
) -> Json<StatusResponse> {
    if payload.domain.is_empty() {
        return Json(StatusResponse {
            status: "error".to_string(),
            message: "Domain cannot be empty".to_string(),
        });
    }

    match state.blocklist.remove_custom_domain(&payload.domain).await {
        Ok(true) => Json(StatusResponse {
            status: "success".to_string(),
            message: format!("Successfully unblocked {}", payload.domain),
        }),
        Ok(false) => Json(StatusResponse {
            status: "not_found".to_string(),
            message: format!("'{}' was not in the custom blocklist", payload.domain),
        }),
        Err(e) => Json(StatusResponse {
            status: "error".to_string(),
            message: format!("Failed to remove domain: {}", e),
        }),
    }
}
```

---

**Step 3 — Register the new route in `src/admin/routes.rs`**

Update the import line at the top:

FROM:
```rust
use crate::admin::domains::{add_custom_domain, list_custom_domains};
```

TO:
```rust
use crate::admin::domains::{add_custom_domain, list_custom_domains, remove_custom_domain};
```

Update the route registration:

FROM:
```rust
.route(
    "/api/v1/domains/custom",
    get(list_custom_domains).post(add_custom_domain),
)
```

TO:
```rust
.route(
    "/api/v1/domains/custom",
    get(list_custom_domains)
        .post(add_custom_domain)
        .delete(remove_custom_domain),
)
```

#### Complete API Surface After This Change

| Method   | Path                        | Body                         | Effect                                   |
|----------|-----------------------------|------------------------------|------------------------------------------|
| `GET`    | `/api/v1/domains/custom`    | —                            | Returns the list of custom blocked domains |
| `POST`   | `/api/v1/domains/custom`    | `{"domain": "ads.example.com"}` | Adds a domain; takes effect immediately |
| `DELETE` | `/api/v1/domains/custom`    | `{"domain": "ads.example.com"}` | Removes a domain; takes effect immediately |

---

### F-2 — Add Graceful Shutdown

**File:** `src/main.rs`

#### The Problem

The server has no shutdown handler. When the process receives `SIGTERM` (from
`systemd stop`, `docker stop`, or `kill`) or the user presses `Ctrl+C`, the process
is terminated immediately. Tokio drops all tasks mid-flight, log output may be
truncated, and any in-progress file write from `add_custom_domain` could be left
partially written.

#### The Fix

Wrap the `axum::serve(...)` call in a `tokio::select!` block that also races against
a `ctrl_c()` signal future. When either the server exits or a shutdown signal is
received, the select block completes, the log message is written, and `main()` returns
`Ok(())` — triggering a clean Tokio runtime shutdown.

**In `src/main.rs`, replace the final two lines of `main()`:**

FROM:
```rust
axum::serve(listener, admin::routes::app(app_state)).await?;

Ok(())
```

TO:
```rust
tokio::select! {
    result = axum::serve(listener, admin::routes::app(app_state)) => {
        if let Err(e) = result {
            error!(error = %e, "Admin server exited with an error");
        }
    }
    _ = tokio::signal::ctrl_c() => {
        info!("Shutdown signal received. Goodbye.");
    }
}

Ok(())
```

`tokio::signal::ctrl_c()` requires no new dependency — it is part of the `signal`
feature already included via `features = ["full"]` in `Cargo.toml`.

#### Why This Works

`tokio::select!` races both futures concurrently. Whichever completes first causes the
entire block to complete. A shutdown signal allows `main()` to return `Ok(())`, which
causes Rust's runtime to run all destructors and flush all buffers before the process
exits. In-flight DNS queries (UDP, stateless) are dropped — this is acceptable for v1
and is consistent with how most DNS servers behave on shutdown.

---

### F-3 — Add Config Validation

**Files:** `src/config.rs`, `src/error.rs`

#### The Problem

Config values are accepted as plain strings with no validation. An invalid
`listen_addr` (e.g. `"not-an-address"`) will not produce a diagnostic until
`UdpSocket::bind()` fails deep in startup with a generic OS error. A missing
`blocklist_path` will not be caught until the file open fails. A typo in `log_level`
(e.g. `"inof"`) is silently accepted by `tracing_subscriber` and produces confusing
filter behaviour with no error.

Explicit validation at load time provides clear, actionable error messages at the
moment of misconfiguration.

#### The Fix

**Step 1 — Add an `InvalidValue` variant to `ConfigError` in `src/error.rs`:**

FROM:
```rust
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse TOML: {0}")]
    Parse(#[from] toml::de::Error),
}
```

TO:
```rust
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse TOML: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Invalid configuration value — {0}")]
    InvalidValue(String),
}
```

**Step 2 — Add a `validate()` method to `Config` in `src/config.rs` and call it from
`load()`:**

```rust
use std::net::ToSocketAddrs;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_dns: String,
    pub blocklist_path: String,
    pub log_level: String,
    #[serde(default = "Config::default_admin_addr")]
    pub admin_addr: String,
}

impl Config {
    fn default_admin_addr() -> String {
        "0.0.0.0:8080".to_string()
    }

    pub fn load(path: &str) -> Result<Self, crate::error::ConfigError> {
        let config_str = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&config_str)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), crate::error::ConfigError> {
        use crate::error::ConfigError::InvalidValue;

        // Validate listen_addr is a parseable socket address
        self.listen_addr
            .to_socket_addrs()
            .map_err(|e| InvalidValue(format!(
                "listen_addr '{}' is not a valid socket address: {}", self.listen_addr, e
            )))?;

        // Validate upstream_dns is a parseable socket address
        self.upstream_dns
            .to_socket_addrs()
            .map_err(|e| InvalidValue(format!(
                "upstream_dns '{}' is not a valid socket address: {}", self.upstream_dns, e
            )))?;

        // Validate admin_addr is a parseable socket address
        self.admin_addr
            .to_socket_addrs()
            .map_err(|e| InvalidValue(format!(
                "admin_addr '{}' is not a valid socket address: {}", self.admin_addr, e
            )))?;

        // Validate log_level is a recognised tracing level
        const VALID_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];
        if !VALID_LEVELS.contains(&self.log_level.to_lowercase().as_str()) {
            return Err(InvalidValue(format!(
                "log_level '{}' is not valid. Must be one of: {}",
                self.log_level,
                VALID_LEVELS.join(", ")
            )));
        }

        // Validate blocklist_path points to an existing file
        if !std::path::Path::new(&self.blocklist_path).exists() {
            return Err(InvalidValue(format!(
                "blocklist_path '{}' does not exist",
                self.blocklist_path
            )));
        }

        Ok(())
    }
}
```

#### Why This Works

`validate()` is called synchronously inside `load()`. Any misconfiguration causes
`main()` to return an error immediately after the first line executes, before any
socket is bound or any file is opened. The user sees a precise message identifying the
exact field and the exact reason it is invalid.

---

## Section 5 — Code Quality Cleanup

---

### C-1 — Restore Commented-Out Domain Logging

**Files:** `src/dns/handler.rs`, `src/dns/packet.rs`

#### The Problem

There are two sections of commented-out code across the DNS handler and packet modules.
Dead code in comments creates ambiguity: was it removed because it was wrong, because
it was a WIP, or because it was accidentally disabled?

The code in question logs the domain name when a query is blocked — one of the most
fundamental and useful pieces of observability an ad blocker can provide. It was
correct; it should be restored, not deleted.

The commented code in `src/dns/handler.rs`:
```rust
// use tracing::info;
// let raw_domain = match dns_packet.get_domain() { ... }
// info!(domain = %raw_domain, status = "BLOCKED", "Query denied by blocklist");
```

The commented code in `src/dns/packet.rs`:
```rust
// pub fn get_domain(&self) -> Option<String> {
//     self.inner.queries().first().map(|query| query.name().to_string())
// }
```

#### The Fix

**Step 1 — Restore `get_domain()` in `src/dns/packet.rs`:**

Uncomment the method so it reads:
```rust
pub fn get_domain(&self) -> Option<String> {
    self.inner
        .queries()
        .first()
        .map(|query| query.name().to_string())
}
```

**Step 2 — Restore the logging in `src/dns/handler.rs`:**

At the top of the file, uncomment or re-add the import:
```rust
use tracing::info;
```

In the `is_blocked` branch, after the `blocked_queries` metric increment and after
parsing the packet, add the log call. The final blocked-path code should look like:

```rust
if state.blocklist.is_blocked(&domain_bytes) {
    state
        .metrics
        .blocked_queries
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let dns_packet = DnsPacket::parse(&packet_bytes)?;

    if let Some(domain) = dns_packet.get_domain() {
        info!(domain = %domain, "BLOCKED");
    }

    let response = dns_packet.make_nxdomain();
    let bytes = response.serialize()?;
    Ok(bytes::Bytes::from(bytes))
}
```

Note that `DnsPacket::parse()` is already called in the original code on the blocked
path; this change simply adds the log line between the parse and the response
construction. There is no additional parsing overhead.

#### Why This Matters

Seeing which domains are being blocked is not optional observability — it is the core
feedback loop of an ad blocker. Users need this to verify that the blocker is working,
to diagnose over-blocking (legitimate sites being blocked), and to understand what
traffic their devices generate. The `info!` call fires only on the blocked path and
has no effect on the hot (allowed) path.

---

## Summary — All Changes to `Cargo.toml`

One new dependency is required. Add it to the `[dependencies]` section:

```toml
ahash = "0.8"
```

The complete `[dependencies]` section after the change:

```toml
[dependencies]
tokio          = { version = "1.30", features = ["full"] }
serde          = { version = "1.0",  features = ["derive"] }
toml           = "0.8"
thiserror      = "1.0"
anyhow         = "1.0"
tracing        = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
axum           = { version = "0.7",  features = ["macros"] }
hickory-proto  = "0.24"
reqwest        = "0.13.2"
bytes          = "1.11.1"
parking_lot    = "0.12.5"
dashmap        = "5.5"
lru            = "0.16.3"
ahash          = "0.8"
```

---

## Recommended Implementation Order

Apply changes in this sequence to minimise the chance of introducing regressions.
Each step is self-contained; you can build and test after each one.

| Step | ID(s)     | Files Touched                                   | Notes                                              |
|------|-----------|-------------------------------------------------|----------------------------------------------------|
| 1    | B-3       | `src/server.rs`                                 | Safest, most isolated fix. Do this first.          |
| 2    | B-1, B-5  | `src/blocklist/mod.rs`                          | Core correctness fix. Also resolves B-5 for free.  |
| 3    | B-2       | `src/blocklist/mod.rs`                          | Same file as step 2 — apply in the same pass.      |
| 4    | B-4, P-1  | `src/blocklist/mod.rs`, `loader.rs`, `handler.rs`, `Cargo.toml` | Same files as above; batch these together. |
| 5    | P-4       | `src/blocklist/mod.rs`                          | Final touch on `blocklist/mod.rs`.                 |
| 6    | B-7, F-3  | `src/config.rs`, `src/error.rs`, `config.toml`  | Add `admin_addr` and validation together.          |
| 7    | B-6       | `src/dns/upstream.rs`, `src/error.rs`           | Small, isolated change.                            |
| 8    | B-8       | `src/main.rs`                                   | One-line fix.                                      |
| 9    | P-2       | `src/blocklist/loader.rs`                       | Isolated to one function.                          |
| 10   | P-3       | `src/dns/cache.rs`, `src/main.rs`               | Small change, easy to verify with timing.          |
| 11   | F-1       | `src/blocklist/mod.rs`, `src/admin/domains.rs`, `src/admin/routes.rs` | New feature; do after all fixes are stable. |
| 12   | F-2       | `src/main.rs`                                   | Final change to `main.rs`.                         |
| 13   | C-1       | `src/dns/handler.rs`, `src/dns/packet.rs`       | Last cleanup pass.                                 |

---

*End of Guide — AdBlocker v1*