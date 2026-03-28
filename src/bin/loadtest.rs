//! rusthole DNS Sinkhole — Load & Throughput Tester
//!
//! Sends real-world DNS A-record queries against the running sinkhole and
//! measures how many it can handle before packet loss climbs above 1%.
//!
//! Workload mirrors real-world traffic:
//!   - 80 % legitimate domains  (search, social, CDN, streaming, cloud, etc.)
//!   - 15 % ad / tracking domains  (typically blocked by StevenBlack list)
//!   -  5 % non-existent domains   (expect NXDOMAIN)
//!
//! Build & run (must be --release for accurate numbers):
//!   cargo build --release --bin loadtest
//!   sudo ./target/release/loadtest             # default: 127.0.0.1:53
//!   sudo ./target/release/loadtest 127.0.0.1:5353

use std::{
    collections::HashMap,
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::{net::UdpSocket, sync::Mutex};

// ─── Real-world domain corpus ────────────────────────────────────────────────
//
// Sourced from Tranco top-1M + known ad/tracker lists.
// Intentionally diverse: different TLDs, CDN subdomains, regional favourites.

static LEGIT_DOMAINS: &[&str] = &[
    // ── Search & portals ────────────────────────────────────────────────────
    "google.com",
    "bing.com",
    "duckduckgo.com",
    "yahoo.com",
    "yandex.com",
    "baidu.com",
    "search.yahoo.com",
    "mail.google.com",
    // ── Social ──────────────────────────────────────────────────────────────
    "facebook.com",
    "twitter.com",
    "instagram.com",
    "linkedin.com",
    "reddit.com",
    "tiktok.com",
    "snapchat.com",
    "pinterest.com",
    "discord.com",
    "telegram.org",
    "signal.org",
    "mastodon.social",
    // ── Video & streaming ────────────────────────────────────────────────────
    "youtube.com",
    "netflix.com",
    "twitch.tv",
    "vimeo.com",
    "hulu.com",
    "disneyplus.com",
    "primevideo.com",
    "hotstar.com",
    "zee5.com",
    "sonyliv.com",
    "crunchyroll.com",
    "funimation.com",
    // ── CDN & infrastructure (very high real-world DNS volume) ───────────────
    "cloudflare.com",
    "cdn.cloudflare.net",
    "fastly.net",
    "akamaiedge.net",
    "akamai.com",
    "cdn.jsdelivr.net",
    "unpkg.com",
    "cdnjs.cloudflare.com",
    "static.cloudflareinsights.com",
    "fonts.googleapis.com",
    "fonts.gstatic.com",
    "ajax.googleapis.com",
    "maps.googleapis.com",
    "maps.gstatic.com",
    "s3.amazonaws.com",
    "d1ge0kk1l5kms0.cloudfront.net",
    "storage.googleapis.com",
    "cdn.shopify.com",
    "assets.github.com",
    "raw.githubusercontent.com",
    // ── Tech / developer ────────────────────────────────────────────────────
    "github.com",
    "api.github.com",
    "stackoverflow.com",
    "gitlab.com",
    "bitbucket.org",
    "npmjs.com",
    "pypi.org",
    "crates.io",
    "hub.docker.com",
    "kubernetes.io",
    "docker.com",
    "circleci.com",
    "github.io",
    "vercel.com",
    "netlify.com",
    "heroku.com",
    "fly.io",
    "render.com",
    "railway.app",
    // ── Cloud ────────────────────────────────────────────────────────────────
    "aws.amazon.com",
    "amazonaws.com",
    "console.cloud.google.com",
    "portal.azure.com",
    "digitalocean.com",
    // ── E-commerce ───────────────────────────────────────────────────────────
    "amazon.com",
    "ebay.com",
    "etsy.com",
    "shopify.com",
    "walmart.com",
    "target.com",
    "aliexpress.com",
    // ── India-popular (realistic for your network location) ─────────────────
    "flipkart.com",
    "meesho.com",
    "swiggy.com",
    "zomato.com",
    "myntra.com",
    "naukri.com",
    "indiamart.com",
    "makemytrip.com",
    "irctc.co.in",
    "icicibank.com",
    "hdfcbank.com",
    "sbi.co.in",
    "axisbank.com",
    "paytm.com",
    "phonepe.com",
    "razorpay.com",
    "jio.com",
    "airtel.in",
    // ── Productivity & SaaS ─────────────────────────────────────────────────
    "slack.com",
    "zoom.us",
    "dropbox.com",
    "notion.so",
    "figma.com",
    "trello.com",
    "asana.com",
    "atlassian.com",
    "jira.com",
    "confluence.atlassian.com",
    "miro.com",
    "airtable.com",
    // ── Finance ──────────────────────────────────────────────────────────────
    "paypal.com",
    "stripe.com",
    "coinbase.com",
    "binance.com",
    // ── News & media ─────────────────────────────────────────────────────────
    "cnn.com",
    "bbc.com",
    "nytimes.com",
    "reuters.com",
    "theguardian.com",
    "bloomberg.com",
    "techcrunch.com",
    "arstechnica.com",
    "wired.com",
    "theverge.com",
    "news.ycombinator.com",
    "medium.com",
    "dev.to",
    "hashnode.com",
    // ── Education ────────────────────────────────────────────────────────────
    "wikipedia.org",
    "khanacademy.org",
    "coursera.org",
    "udemy.com",
    "edx.org",
    "duolingo.com",
    "quora.com",
    "wolframalpha.com",
    "archive.org",
    // ── Gaming ───────────────────────────────────────────────────────────────
    "steampowered.com",
    "store.steampowered.com",
    "epicgames.com",
    "ea.com",
    "battlenet.com",
    "xbox.com",
    "playstation.com",
    "roblox.com",
    "minecraft.net",
    "leagueoflegends.com",
    // ── Apple / Microsoft ecosystem ──────────────────────────────────────────
    "apple.com",
    "icloud.com",
    "microsoft.com",
    "office.com",
    "outlook.com",
    "live.com",
    "msn.com",
    "bing.com",
    "spotify.com",
    "soundcloud.com",
    "lastfm.com",
];

static AD_DOMAINS: &[&str] = &[
    // These are all in the StevenBlack hosts file → rusthole blocks them
    "doubleclick.net",
    "googleadservices.com",
    "googlesyndication.com",
    "google-analytics.com",
    "adservice.google.com",
    "advertising.com",
    "adnxs.com",
    "media.net",
    "criteo.com",
    "outbrain.com",
    "taboola.com",
    "moatads.com",
    "pubmatic.com",
    "rubiconproject.com",
    "openx.net",
    "appnexus.com",
    "smartadserver.com",
    "inmobi.com",
    "ads.yahoo.com",
    "scorecardresearch.com",
];

static NX_DOMAINS: &[&str] = &[
    // Should elicit NXDOMAIN responses
    "nonexistent-domain-xyz-rusthole-loadtest.com",
    "fake-domain-abc-123-rusthole-loadtest.org",
    "no-such-domain-rusthole-test-9999.net",
    "totally-fake-xyz-abc-rusthole-test.io",
    "this-does-not-exist-rusthole-loadtest.co",
];

/// Returns a domain name for query index `i` with the correct workload ratio.
#[inline]
fn get_domain(i: usize) -> &'static str {
    match i % 20 {
        0..=15 => LEGIT_DOMAINS[i % LEGIT_DOMAINS.len()], // 80 %
        16..=18 => AD_DOMAINS[i % AD_DOMAINS.len()],      // 15 %
        _ => NX_DOMAINS[i % NX_DOMAINS.len()],            //  5 %
    }
}

// ─── DNS wire-format query builder ──────────────────────────────────────────

/// Builds a minimal valid DNS A-record query packet.
/// `id = 0` is fine for templates; callers overwrite bytes [0..2].
fn build_dns_query(id: u16, domain: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&id.to_be_bytes()); // ID
    buf.extend_from_slice(&[0x01, 0x00]); // Flags: RD=1
    buf.extend_from_slice(&[0x00, 0x01]); // QDCOUNT = 1
    buf.extend_from_slice(&[0x00, 0x00]); // ANCOUNT = 0
    buf.extend_from_slice(&[0x00, 0x00]); // NSCOUNT = 0
    buf.extend_from_slice(&[0x00, 0x00]); // ARCOUNT = 0
    for label in domain.split('.') {
        // QNAME
        let b = label.as_bytes();
        buf.push(b.len() as u8);
        buf.extend_from_slice(b);
    }
    buf.push(0x00); // root label
    buf.extend_from_slice(&[0x00, 0x01]); // QTYPE  = A
    buf.extend_from_slice(&[0x00, 0x01]); // QCLASS = IN
    buf
}

// ─── Stats ───────────────────────────────────────────────────────────────────

#[derive(Default, Debug)]
struct Stats {
    sent: AtomicU64,
    received: AtomicU64,
    errors: AtomicU64,
}

// ─── Phase result ─────────────────────────────────────────────────────────────

struct PhaseResult {
    target_rps: u64,
    achieved_rps: f64,
    sent: u64,
    received: u64,
    errors: u64,
    drop_pct: f64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    sample_count: usize,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ─── Core load phase ──────────────────────────────────────────────────────────
//
// Design:
//   - SOCKET_COUNT independent UDP sockets (avoids single-socket bottleneck,
//     spreads kernel load across multiple file descriptors).
//   - Each socket owns a sender task and a receiver task sharing via Arc.
//   - Senders fire packets in bursts of BATCH_SIZE, then sleep to hit the
//     per-socket rate target.  Burst-sleep is far more tokio-scheduler-friendly
//     than per-packet sleeps at μs granularity.
//   - Every SAMPLE_EVERY-th packet (by DNS-ID) is tracked in a pending map so
//     we can compute end-to-end latency without paying HashMap overhead at
//     full line rate.
//   - A progress monitor prints a heartbeat line every 3 s.

const SOCKET_COUNT: usize = 8;
const BATCH_SIZE: u64 = 64; // packets per burst
const SAMPLE_EVERY: u16 = 50; // latency sample rate  (1-in-50 = 2 %)

async fn run_phase(addr: SocketAddr, target_rps: u64, duration: Duration) -> PhaseResult {
    let stats = Arc::new(Stats::default());
    let lats: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

    let per_socket_rps = (target_rps / SOCKET_COUNT as u64).max(1);
    // How long to sleep between batches so we hit per_socket_rps.
    // batch_sleep = BATCH_SIZE / per_socket_rps  (converted to µs)
    let batch_sleep_us = BATCH_SIZE * 1_000_000 / per_socket_rps;
    let batch_sleep = Duration::from_micros(batch_sleep_us);

    let deadline = Instant::now() + duration;
    let mut handles = Vec::with_capacity(SOCKET_COUNT * 2 + 1);

    // Pre-build one query template per unique domain (reuse, only patch ID).
    // Domain count = LEGIT + AD + NX  (~145 entries).  Tiny allocation.
    let domain_count = LEGIT_DOMAINS.len() + AD_DOMAINS.len() + NX_DOMAINS.len();
    let templates: Arc<Vec<Vec<u8>>> = Arc::new(
        (0..domain_count)
            .map(|i| build_dns_query(0, get_domain(i)))
            .collect(),
    );

    // ── Progress monitor ──────────────────────────────────────────────────
    {
        let stats = stats.clone();
        handles.push(tokio::spawn(async move {
            let start = Instant::now();
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                if Instant::now() >= deadline {
                    break;
                }
                let elapsed = start.elapsed().as_secs_f64();
                let s = stats.sent.load(Ordering::Relaxed);
                let r = stats.received.load(Ordering::Relaxed);
                let cur_rps = s as f64 / elapsed;
                let drop = if s > 0 {
                    (1.0 - r as f64 / s as f64) * 100.0
                } else {
                    0.0
                };
                println!(
                    "    [{:>4.0}s]  sent={:>9}  recv={:>9}  rps={:>8.0}  drop={:>5.2}%",
                    elapsed, s, r, cur_rps, drop
                );
            }
        }));
    }

    for socket_idx in 0..SOCKET_COUNT {
        let socket = Arc::new(
            UdpSocket::bind("0.0.0.0:0")
                .await
                .expect("bind failed — check permissions"),
        );
        socket.connect(addr).await.expect("connect failed");

        // Shared pending map: DNS-ID → send Instant (sampled packets only).
        // At 2 % sampling, ~250 entries/sec per socket — lock contention is negligible.
        let pending: Arc<Mutex<HashMap<u16, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

        // ── Receiver ──────────────────────────────────────────────────────
        {
            let socket = socket.clone();
            let stats = stats.clone();
            let pending = pending.clone();
            let lats = lats.clone();
            // Receive for 300 ms past the deadline to drain in-flight responses.
            let recv_deadline = deadline + Duration::from_millis(300);

            handles.push(tokio::spawn(async move {
                let mut buf = vec![0u8; 512];
                while Instant::now() < recv_deadline {
                    let remaining = recv_deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(100));

                    match tokio::time::timeout(remaining, socket.recv(&mut buf)).await {
                        Ok(Ok(len)) if len >= 2 => {
                            stats.received.fetch_add(1, Ordering::Relaxed);
                            let id = u16::from_be_bytes([buf[0], buf[1]]);
                            // Only lock if this ID might be in the pending map.
                            if id % SAMPLE_EVERY == 0 {
                                if let Some(sent_at) = pending.lock().await.remove(&id) {
                                    let us = sent_at.elapsed().as_micros() as u64;
                                    lats.lock().await.push(us);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }));
        }

        // ── Sender ────────────────────────────────────────────────────────
        {
            let socket = socket.clone();
            let stats = stats.clone();
            let pending = pending.clone();
            let templates = templates.clone();

            handles.push(tokio::spawn(async move {
                // Stagger starting domain index and DNS-ID per socket so
                // each socket hits different domains — no thundering herd.
                let mut domain_idx: usize = socket_idx * 997;
                let mut dns_id: u16 = (socket_idx as u16).wrapping_mul(8191);

                while Instant::now() < deadline {
                    let batch_start = Instant::now();

                    for _ in 0..BATCH_SIZE {
                        let id = dns_id;
                        dns_id = dns_id.wrapping_add(1);

                        let tmpl_idx = domain_idx % templates.len();
                        domain_idx = domain_idx.wrapping_add(1);

                        // Clone template and patch the ID field.
                        let mut pkt = templates[tmpl_idx].clone();
                        pkt[0] = (id >> 8) as u8;
                        pkt[1] = id as u8;

                        if id % SAMPLE_EVERY == 0 {
                            pending.lock().await.insert(id, Instant::now());
                        }

                        match socket.send(&pkt).await {
                            Ok(_) => {
                                stats.sent.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(_) => {
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }

                    let spent = batch_start.elapsed();
                    if spent < batch_sleep {
                        tokio::time::sleep(batch_sleep - spent).await;
                    }
                    // If we're already over budget, loop immediately (catch-up).
                }
            }));
        }
    }

    // Wait for all tasks to finish.
    for h in handles {
        let _ = h.await;
    }

    // ── Compute results ───────────────────────────────────────────────────
    let sent = stats.sent.load(Ordering::Relaxed);
    let received = stats.received.load(Ordering::Relaxed);
    let errors = stats.errors.load(Ordering::Relaxed);
    let drop_pct = if sent > 0 {
        (1.0 - received as f64 / sent as f64).max(0.0) * 100.0
    } else {
        0.0
    };
    let achieved_rps = sent as f64 / duration.as_secs_f64();

    let mut lats = lats.lock().await;
    lats.sort_unstable();
    let sample_count = lats.len();
    let p50 = percentile(&lats, 50.0);
    let p95 = percentile(&lats, 95.0);
    let p99 = percentile(&lats, 99.0);

    PhaseResult {
        target_rps,
        achieved_rps,
        sent,
        received,
        errors,
        drop_pct,
        p50_us: p50,
        p95_us: p95,
        p99_us: p99,
        sample_count,
    }
}

// ─── Pretty printer ──────────────────────────────────────────────────────────

fn print_result(r: &PhaseResult) {
    let verdict = if r.drop_pct < 1.0 {
        "✓ WITHIN LIMIT"
    } else if r.drop_pct < 5.0 {
        "⚠ DEGRADED"
    } else {
        "✗ OVERLOADED"
    };
    println!("  Verdict     : {verdict}");
    println!(
        "  Target RPS  : {:>10}    Achieved RPS : {:>10.0}",
        fmt_u64(r.target_rps),
        r.achieved_rps
    );
    println!(
        "  Sent        : {:>10}    Received     : {:>10}    Errors: {}",
        fmt_u64(r.sent),
        fmt_u64(r.received),
        r.errors
    );
    println!("  Drop Rate   : {:>9.3}%", r.drop_pct);
    println!(
        "  Latency     : p50 = {:.2} ms   p95 = {:.2} ms   p99 = {:.2} ms   ({} samples)",
        r.p50_us as f64 / 1_000.0,
        r.p95_us as f64 / 1_000.0,
        r.p99_us as f64 / 1_000.0,
        r.sample_count,
    );
    println!();
}

fn fmt_u64(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.chars().rev().collect()
}

// ─── Binary search for max sustainable RPS ───────────────────────────────────

const DROP_THRESHOLD: f64 = 1.0; // % — above this is "overloaded"
const SEARCH_DURATION: Duration = Duration::from_secs(10);

async fn find_max_rps(addr: SocketAddr) -> u64 {
    let mut lo: u64 = 5_000;
    let mut hi: u64 = 300_000;

    println!("─── Binary Search: Finding Max Sustainable RPS ─────────────────────────────");
    println!(
        "  Threshold: < {:.0}% packet loss    Probe duration: {}s per step",
        DROP_THRESHOLD,
        SEARCH_DURATION.as_secs()
    );
    println!("  Range: {} – {} RPS", fmt_u64(lo), fmt_u64(hi));
    println!();

    for round in 1..=9u32 {
        let mid = (lo + hi) / 2;
        print!("  Step {:>2} | {:>9} RPS | ", round, fmt_u64(mid));
        // Flush stdout so the line appears before the 10-second wait.
        use std::io::Write;
        let _ = std::io::stdout().flush();

        let r = run_phase(addr, mid, SEARCH_DURATION).await;

        if r.drop_pct < DROP_THRESHOLD {
            println!(
                "drop = {:>5.2}%   p99 = {:>6.1} ms   ✓ sustainable  → raise floor",
                r.drop_pct,
                r.p99_us as f64 / 1_000.0
            );
            lo = mid;
        } else {
            println!(
                "drop = {:>5.2}%   p99 = {:>6.1} ms   ✗ dropping     → lower ceiling",
                r.drop_pct,
                r.p99_us as f64 / 1_000.0
            );
            hi = mid;
        }

        if hi.saturating_sub(lo) < 2_000 {
            println!("  Converged (Δ < 2,000 RPS)");
            break;
        }
    }

    lo
}

// ─── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let addr_str = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:53".to_string());

    let addr = match SocketAddr::from_str(&addr_str) {
        Ok(a) => a,
        Err(_) => {
            eprintln!("ERROR: '{}' is not a valid socket address.", addr_str);
            eprintln!("Usage: loadtest [ip:port]   e.g. 127.0.0.1:5353");
            std::process::exit(1);
        }
    };

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║           rusthole DNS Sinkhole — Throughput Load Test              ║");
    println!("╠══════════════════════════════════════════════════════════════════════╣");
    println!("║  Target  : {:<58} ║", addr);
    println!(
        "║  Sockets : {:<3}    Batch : {:<3}    Latency sample : 1 in {:<14} ║",
        SOCKET_COUNT, BATCH_SIZE, SAMPLE_EVERY
    );
    println!("║  Workload: 80 % legit  │  15 % ad/tracking  │  5 % NXDOMAIN        ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();

    // ── Phase 1 : Warm-up ──────────────────────────────────────────────────
    println!("─── Phase 1 : Warm-up  (10,000 RPS × 5 s) ─────────────────────────────────");
    let r = run_phase(addr, 10_000, Duration::from_secs(5)).await;
    print_result(&r);

    // ── Phase 2 : Moderate Load ────────────────────────────────────────────
    println!("─── Phase 2 : Moderate Load  (50,000 RPS × 15 s) ──────────────────────────");
    let r = run_phase(addr, 50_000, Duration::from_secs(15)).await;
    print_result(&r);

    // ── Phase 3 : Stress — 100 k RPS ──────────────────────────────────────
    println!("─── Phase 3 : Stress Test  (100,000 RPS × 15 s) ───────────────────────────");
    let r = run_phase(addr, 100_000, Duration::from_secs(15)).await;
    print_result(&r);

    // ── Phase 4 : Binary search for ceiling ────────────────────────────────
    println!();
    let max_rps = find_max_rps(addr).await;

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!(
        "║   Max Sustainable RPS  ≈  {:>12}                              ║",
        fmt_u64(max_rps)
    );
    println!(
        "║   (sustained < {:.0}% packet loss over {}-second probes)           ║",
        DROP_THRESHOLD,
        SEARCH_DURATION.as_secs()
    );
    println!("╚══════════════════════════════════════════════════════════════════════╝");
    println!();
    println!("  Tip: re-run with a higher starting ceiling if the binary search");
    println!("       converged at the upper bound (300,000), which means rusthole");
    println!("       can sustain even more — increase `hi` in find_max_rps().");
    println!();
}
