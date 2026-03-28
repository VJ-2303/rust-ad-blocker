//! rusthole DNS Sinkhole — Production Load & Throughput Tester
//!
//! Downloads real-world domain lists from Majestic Million at startup.
//! Simulates realistic traffic: 75% legit, 15% ad/tracking (blocked), 10% NX.
//! Live ANSI terminal dashboard. Binary-searches for maximum sustainable RPS.
//!
//! Build & run:
//!   cargo build --release --bin loadtest
//!   sudo ./target/release/loadtest
//!   sudo ./target/release/loadtest 127.0.0.1:5353

#![allow(dead_code)]
use std::{
    collections::HashMap,
    io::{self, Write},
    net::SocketAddr,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::{net::UdpSocket, sync::Mutex};

// ═══════════════════════════════════════════════════════════════════════════════
// ANSI Terminal Codes
// ═══════════════════════════════════════════════════════════════════════════════

const RST: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[91m";
const GREEN: &str = "\x1b[92m";
const YELLOW: &str = "\x1b[93m";
const BLUE: &str = "\x1b[94m";
const MAGENTA: &str = "\x1b[95m";
const CYAN: &str = "\x1b[96m";
const WHITE: &str = "\x1b[97m";
const GREY: &str = "\x1b[90m";

const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";
const CLEAR_DOWN: &str = "\x1b[J";

fn up(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("\x1b[{}A", n)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Known Ad / Tracker Domains  (StevenBlack hosts list — rusthole blocks these)
// ═══════════════════════════════════════════════════════════════════════════════

static BLOCKED_DOMAINS: &[&str] = &[
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
    "quantserve.com",
    "adsafeprotected.com",
    "adzerk.net",
    "amazon-adsystem.com",
    "bidswitch.net",
    "bluekai.com",
    "casalemedia.com",
    "demdex.net",
    "dotomi.com",
    "exponential.com",
    "flashtalking.com",
    "lijit.com",
    "lotame.com",
    "sharethrough.com",
    "spotxchange.com",
    "turn.com",
    "undertone.com",
    "yieldmanager.com",
    "zedo.com",
    "tracker.com",
    "analytics.com",
    "adform.net",
    "adalyser.com",
    "adcolony.com",
    "adtech.com",
    "contextweb.com",
    "conversant.com",
    "doubleverify.com",
    "eyeota.net",
    "fundingchoicesmessages.google.com",
];

static NX_DOMAINS: &[&str] = &[
    "this-domain-absolutely-does-not-exist-rusthole-001.com",
    "fake-nxdomain-rusthole-benchmark-test-002.org",
    "nonexistent-dns-rusthole-perf-test-003.net",
    "no-such-domain-rusthole-loadtest-004.io",
    "totally-fake-rusthole-dns-test-005.co",
    "invalid-domain-rusthole-test-006.dev",
    "ghost-domain-rusthole-bench-007.app",
    "null-domain-rusthole-load-008.xyz",
    "missing-rusthole-test-domain-009.me",
    "void-domain-rusthole-perf-010.info",
];

// ═══════════════════════════════════════════════════════════════════════════════
// Domain Fetcher — Real-world lists from the internet
// ═══════════════════════════════════════════════════════════════════════════════

/// Downloads the Majestic Million top-N domains.
/// CSV format: GlobalRank,TldRank,Domain,TLD,...
/// Falls back to Umbrella top 1M, then to built-in corpus.
pub async fn fetch_legit_domains(want: usize) -> (Vec<String>, &'static str) {
    // Source 1: Majestic Million — plain CSV, no decompression needed
    match fetch_majestic(want).await {
        Ok(v) if v.len() >= 200 => return (v, "Majestic Million"),
        Ok(v) if !v.is_empty() => return (v, "Majestic Million (partial)"),
        _ => {}
    }

    // Source 2: Cisco Umbrella top 1M (CSV in a zip — parse as text, skip zip header)
    match fetch_umbrella(want).await {
        Ok(v) if v.len() >= 200 => return (v, "Cisco Umbrella"),
        _ => {}
    }

    // Fallback: built-in corpus
    (builtin_corpus(), "built-in corpus")
}

async fn fetch_majestic(
    want: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(25))
        .user_agent("rusthole-loadtest/1.0")
        .build()?;

    let resp = client
        .get("https://downloads.majestic.com/majestic_million.csv")
        .send()
        .await?;

    let text = resp.text().await?;
    let mut domains = Vec::with_capacity(want);

    for line in text.lines().skip(1) {
        // GlobalRank,TldRank,Domain,TLD,RefSubNets,...
        let mut cols = line.splitn(4, ',');
        cols.next(); // GlobalRank
        cols.next(); // TldRank
        if let Some(domain) = cols.next() {
            let d = domain.trim();
            if !d.is_empty() && d.contains('.') && !d.starts_with('#') && d.len() <= 253 {
                domains.push(d.to_string());
                if domains.len() >= want {
                    break;
                }
            }
        }
    }
    Ok(domains)
}

async fn fetch_umbrella(
    want: usize,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    // Tranco API returns JSON — parse list_id from plain text to avoid serde_json dep
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("rusthole-loadtest/1.0")
        .build()?;

    let resp = client
        .get("https://tranco-list.eu/api/lists/date/latest")
        .send()
        .await?;

    let body = resp.text().await?;

    // Extract "list_id":"XXXX" with simple string parsing — no serde_json needed
    let list_id = body
        .split("\"list_id\":")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
        .ok_or("Tranco: could not find list_id in API response")?
        .to_string();

    let list_url = format!("https://tranco-list.eu/download/{}/100000", list_id);
    let resp2 = client.get(&list_url).send().await?;
    let text = resp2.text().await?;

    let mut domains = Vec::with_capacity(want);
    for line in text.lines() {
        // Tranco CSV format: rank,domain
        if let Some(domain) = line.splitn(2, ',').nth(1) {
            let d = domain.trim();
            if !d.is_empty() && d.contains('.') {
                domains.push(d.to_string());
                if domains.len() >= want {
                    break;
                }
            }
        }
    }
    Ok(domains)
}

fn builtin_corpus() -> Vec<String> {
    let raw: &[&str] = &[
        // ── Search & portals ────────────────────────────────────────────────
        "google.com",
        "bing.com",
        "duckduckgo.com",
        "yahoo.com",
        "yandex.com",
        "baidu.com",
        "search.yahoo.com",
        "ecosia.org",
        "startpage.com",
        "brave.com",
        // ── Social ─────────────────────────────────────────────────────────
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
        "threads.net",
        "bluesky.app",
        "tumblr.com",
        "twitch.tv",
        // ── Video & streaming ───────────────────────────────────────────────
        "youtube.com",
        "netflix.com",
        "vimeo.com",
        "hulu.com",
        "disneyplus.com",
        "primevideo.com",
        "hotstar.com",
        "zee5.com",
        "sonyliv.com",
        "crunchyroll.com",
        "dailymotion.com",
        "rumble.com",
        "peacocktv.com",
        "paramountplus.com",
        "max.com",
        "appletv.apple.com",
        // ── CDN & infrastructure ────────────────────────────────────────────
        "cloudflare.com",
        "fastly.net",
        "akamai.com",
        "akamaiedge.net",
        "cdn.jsdelivr.net",
        "unpkg.com",
        "cdnjs.cloudflare.com",
        "fonts.googleapis.com",
        "fonts.gstatic.com",
        "ajax.googleapis.com",
        "maps.googleapis.com",
        "maps.gstatic.com",
        "s3.amazonaws.com",
        "cloudfront.net",
        "storage.googleapis.com",
        "cdn.shopify.com",
        "assets.github.com",
        "raw.githubusercontent.com",
        "objects.githubusercontent.com",
        "static.cloudflareinsights.com",
        "d1ge0kk1l5kms0.cloudfront.net",
        // ── Developer & tech ────────────────────────────────────────────────
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
        "supabase.com",
        "planetscale.com",
        "neon.tech",
        "hackernews.com",
        "news.ycombinator.com",
        "lobste.rs",
        "dev.to",
        "hashnode.com",
        "medium.com",
        "substack.com",
        "ghost.org",
        // ── Cloud providers ─────────────────────────────────────────────────
        "aws.amazon.com",
        "amazonaws.com",
        "cloud.google.com",
        "azure.com",
        "portal.azure.com",
        "digitalocean.com",
        "linode.com",
        "vultr.com",
        "hetzner.com",
        "ovh.com",
        "scaleway.com",
        "cloudflare.com",
        // ── E-commerce ──────────────────────────────────────────────────────
        "amazon.com",
        "ebay.com",
        "etsy.com",
        "shopify.com",
        "walmart.com",
        "target.com",
        "aliexpress.com",
        "alibaba.com",
        "wish.com",
        "bestbuy.com",
        "costco.com",
        "wayfair.com",
        "chewy.com",
        // ── India-popular ───────────────────────────────────────────────────
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
        "bsnl.in",
        "ola.com",
        "bigbasket.com",
        "1mg.com",
        "lenskart.com",
        "pepperfry.com",
        "policybazaar.com",
        "bookmyshow.com",
        "sharechat.com",
        "moj.app",
        "josh.app",
        // ── Productivity & SaaS ─────────────────────────────────────────────
        "slack.com",
        "zoom.us",
        "dropbox.com",
        "notion.so",
        "figma.com",
        "trello.com",
        "asana.com",
        "atlassian.com",
        "jira.com",
        "miro.com",
        "airtable.com",
        "salesforce.com",
        "hubspot.com",
        "zendesk.com",
        "intercom.com",
        "freshdesk.com",
        "calendly.com",
        "loom.com",
        "linear.app",
        "clickup.com",
        "basecamp.com",
        "monday.com",
        // ── Finance ─────────────────────────────────────────────────────────
        "paypal.com",
        "stripe.com",
        "coinbase.com",
        "binance.com",
        "kraken.com",
        "robinhood.com",
        "etrade.com",
        "fidelity.com",
        "vanguard.com",
        "schwab.com",
        "wise.com",
        "revolut.com",
        // ── News & media ────────────────────────────────────────────────────
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
        "engadget.com",
        "gizmodo.com",
        "mashable.com",
        "thehindu.com",
        "ndtv.com",
        "timesofindia.com",
        "hindustantimes.com",
        "deccanherald.com",
        "indianexpress.com",
        "scroll.in",
        "thewire.in",
        // ── Education ───────────────────────────────────────────────────────
        "wikipedia.org",
        "khanacademy.org",
        "coursera.org",
        "udemy.com",
        "edx.org",
        "duolingo.com",
        "quora.com",
        "wolframalpha.com",
        "archive.org",
        "gutenberg.org",
        "academia.edu",
        "researchgate.net",
        "springer.com",
        "nature.com",
        "science.org",
        "ieee.org",
        "acm.org",
        // ── Gaming ──────────────────────────────────────────────────────────
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
        "ubisoft.com",
        "activision.com",
        "riot.games",
        "valvesoftware.com",
        "itch.io",
        // ── Apple / Microsoft ────────────────────────────────────────────────
        "apple.com",
        "icloud.com",
        "microsoft.com",
        "office.com",
        "outlook.com",
        "live.com",
        "msn.com",
        "office365.com",
        "teams.microsoft.com",
        "onedrive.live.com",
        "sharepoint.com",
        // ── Music ────────────────────────────────────────────────────────────
        "spotify.com",
        "soundcloud.com",
        "tidal.com",
        "deezer.com",
        "pandora.com",
        "last.fm",
        "bandcamp.com",
        "audiomack.com",
        // ── Web infra ────────────────────────────────────────────────────────
        "wordpress.com",
        "wix.com",
        "squarespace.com",
        "webflow.com",
        "godaddy.com",
        "namecheap.com",
        "dynadot.com",
        "porkbun.com",
        "letsencrypt.org",
        "mozilla.org",
        "gnu.org",
        "kernel.org",
        "debian.org",
        "ubuntu.com",
        "fedoraproject.org",
        "archlinux.org",
    ];
    raw.iter().map(|s| s.to_string()).collect()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Formatting Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn fmt_num(n: u64) -> String {
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

fn fmt_ms(us: u64) -> String {
    if us == 0 {
        "  --  ".to_string()
    } else if us < 1_000 {
        format!("{:>5}µs", us)
    } else {
        format!("{:>5.1}ms", us as f64 / 1_000.0)
    }
}

fn fmt_pct(p: f64) -> String {
    format!("{:.2}%", p)
}

fn progress_bar(pct: f64, width: usize) -> String {
    let clamped = pct.clamp(0.0, 100.0);
    let filled = ((clamped / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width - filled;
    format!(
        "{GREEN}{}{GREY}{}{RST}",
        "█".repeat(filled),
        "░".repeat(empty)
    )
}

fn verdict(drop_pct: f64) -> (&'static str, &'static str) {
    if drop_pct < 1.0 {
        ("✓ WITHIN LIMIT", GREEN)
    } else if drop_pct < 5.0 {
        ("⚠ DEGRADED   ", YELLOW)
    } else {
        ("✗ OVERLOADED ", RED)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DNS Wire-Format Builder
// ═══════════════════════════════════════════════════════════════════════════════

fn build_dns_query(id: u16, domain: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    buf.extend_from_slice(&id.to_be_bytes()); // ID
    buf.extend_from_slice(&[0x01, 0x00]); // Flags: RD=1
    buf.extend_from_slice(&[0x00, 0x01]); // QDCOUNT = 1
    buf.extend_from_slice(&[0x00, 0x00]); // ANCOUNT = 0
    buf.extend_from_slice(&[0x00, 0x00]); // NSCOUNT = 0
    buf.extend_from_slice(&[0x00, 0x00]); // ARCOUNT = 0
    for label in domain.split('.') {
        let b = label.as_bytes();
        if !b.is_empty() && b.len() <= 63 {
            buf.push(b.len() as u8);
            buf.extend_from_slice(b);
        }
    }
    buf.push(0x00); // Root label
    buf.extend_from_slice(&[0x00, 0x01]); // QTYPE  = A
    buf.extend_from_slice(&[0x00, 0x01]); // QCLASS = IN
    buf
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared Live Stats
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Default)]
struct LiveStats {
    sent: AtomicU64,
    received: AtomicU64,
    errors: AtomicU64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase Result
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Clone)]
struct PhaseResult {
    name: String,
    target_rps: u64,
    achieved_rps: f64,
    sent: u64,
    received: u64,
    errors: u64,
    drop_pct: f64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ═══════════════════════════════════════════════════════════════════════════════
// Load Phase Runner
// ═══════════════════════════════════════════════════════════════════════════════

const SOCKET_COUNT: usize = 8;
const BATCH_SIZE: u64 = 4;
const SAMPLE_EVERY: u16 = 40; // 1-in-40 ≈ 2.5% latency sampling

/// Returns the domain to query for a given counter index.
/// Traffic mix: 75% legit · 15% blocked · 10% NX
fn pick_domain<'a>(i: usize, legit: &'a [String]) -> &'a str {
    match i % 20 {
        0..=14 => &legit[i % legit.len()],
        15..=17 => BLOCKED_DOMAINS[i % BLOCKED_DOMAINS.len()],
        _ => NX_DOMAINS[i % NX_DOMAINS.len()],
    }
}

struct PhaseRun {
    addr: SocketAddr,
    target_rps: u64,
    duration: Duration,
    legit_domains: Arc<Vec<String>>,
    stats: Arc<LiveStats>,
    latencies: Arc<Mutex<Vec<u64>>>,
}

async fn run_phase(cfg: &PhaseRun) {
    let per_socket_rps = (cfg.target_rps / SOCKET_COUNT as u64).max(1);
    let batch_sleep_us = BATCH_SIZE * 1_000_000 / per_socket_rps;
    let batch_sleep = Duration::from_micros(batch_sleep_us);
    let deadline = Instant::now() + cfg.duration;

    // Pre-build query templates — one per domain in corpus.
    // At runtime we only patch the 2-byte ID field; avoids repeated allocation.
    let domain_count = cfg.legit_domains.len() + BLOCKED_DOMAINS.len() + NX_DOMAINS.len();
    let templates: Arc<Vec<Vec<u8>>> = Arc::new(
        (0..domain_count)
            .map(|i| {
                let domain = pick_domain(i, &cfg.legit_domains);
                build_dns_query(0, domain)
            })
            .collect(),
    );

    let mut handles = Vec::with_capacity(SOCKET_COUNT * 2);

    for socket_idx in 0..SOCKET_COUNT {
        let socket = Arc::new(
            UdpSocket::bind("0.0.0.0:0")
                .await
                .expect("bind failed — need CAP_NET_BIND_SERVICE or root"),
        );
        socket.connect(cfg.addr).await.expect("connect failed");

        // Pending map: rewritten DNS ID → send Instant (sampled only).
        // ~2.5% sampling means ~600 entries/sec/socket — lock contention negligible.
        let pending: Arc<Mutex<HashMap<u16, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

        // ── Receiver task ────────────────────────────────────────────────
        {
            let socket = socket.clone();
            let stats = cfg.stats.clone();
            let pending = pending.clone();
            let latencies = cfg.latencies.clone();
            let recv_deadline = deadline + Duration::from_millis(500);

            handles.push(tokio::spawn(async move {
                let mut buf = vec![0u8; 512];
                while Instant::now() < recv_deadline {
                    let timeout = recv_deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_millis(100));

                    match tokio::time::timeout(timeout, socket.recv(&mut buf)).await {
                        Ok(Ok(n)) if n >= 2 => {
                            stats.received.fetch_add(1, Ordering::Relaxed);
                            let id = u16::from_be_bytes([buf[0], buf[1]]);
                            if id % SAMPLE_EVERY == 0 {
                                if let Some(t) = pending.lock().await.remove(&id) {
                                    latencies.lock().await.push(t.elapsed().as_micros() as u64);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }));
        }

        // ── Sender task ──────────────────────────────────────────────────
        {
            let socket = socket.clone();
            let stats = cfg.stats.clone();
            let pending = pending.clone();
            let templates = templates.clone();

            handles.push(tokio::spawn(async move {
                // Stagger starting positions per socket to avoid thundering herd
                let mut domain_idx: usize = socket_idx.wrapping_mul(997);
                let mut dns_id: u16 = (socket_idx as u16).wrapping_mul(8191);

                while Instant::now() < deadline {
                    let batch_start = Instant::now();

                    for _ in 0..BATCH_SIZE {
                        let id = dns_id;
                        dns_id = dns_id.wrapping_add(1);

                        let tmpl_idx = domain_idx % templates.len();
                        domain_idx = domain_idx.wrapping_add(1);

                        // Clone template, patch ID in-place
                        let mut pkt = templates[tmpl_idx].clone();
                        pkt[0] = (id >> 8) as u8;
                        pkt[1] = id as u8;

                        // Record send time for sampled IDs
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
                    // If we're over budget, loop immediately (catch-up mode)
                }
            }));
        }
    }

    for h in handles {
        let _ = h.await;
    }
}

async fn execute_phase(
    name: &str,
    addr: SocketAddr,
    target_rps: u64,
    duration: Duration,
    legit_domains: Arc<Vec<String>>,
    history: &[PhaseResult],
    phase_idx: usize,
    phase_total: usize,
    domain_count: usize,
) -> PhaseResult {
    let stats = Arc::new(LiveStats::default());
    let latencies: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

    let cfg = Arc::new(PhaseRun {
        addr,
        target_rps,
        duration,
        legit_domains,
        stats: stats.clone(),
        latencies: latencies.clone(),
    });

    // Spawn the actual load runner
    let run_cfg = cfg.clone();
    let phase_handle = tokio::spawn(async move {
        run_phase(&run_cfg).await;
    });

    // ── Live dashboard refresh loop ──────────────────────────────────────
    let refresh = Duration::from_millis(200);
    let phase_start = Instant::now();
    let mut last_lines: usize = 0;
    let mut stdout = io::stdout();

    loop {
        let elapsed = phase_start.elapsed();
        let done = elapsed >= duration + Duration::from_millis(600);

        let sent = stats.sent.load(Ordering::Relaxed);
        let received = stats.received.load(Ordering::Relaxed);
        let errors = stats.errors.load(Ordering::Relaxed);
        let drop_pct = if sent > 0 {
            (1.0f64 - received as f64 / sent as f64).max(0.0) * 100.0
        } else {
            0.0
        };
        let achieved_rps = sent as f64 / elapsed.as_secs_f64().max(0.001);

        // Sample current latency percentiles without locking for long
        let (p50, p95, p99) = {
            let mut lats = latencies.lock().await;
            if lats.len() >= 4 {
                lats.sort_unstable();
                (
                    percentile(&lats, 50.0),
                    percentile(&lats, 95.0),
                    percentile(&lats, 99.0),
                )
            } else {
                (0, 0, 0)
            }
        };

        let draw_elapsed = if done {
            duration
        } else {
            elapsed.min(duration)
        };

        let block = render_phase_block(
            name,
            phase_idx,
            phase_total,
            target_rps,
            duration,
            draw_elapsed,
            sent,
            received,
            errors,
            drop_pct,
            achieved_rps,
            p50,
            p95,
            p99,
            history,
            domain_count,
        );

        let line_count = block.chars().filter(|&c| c == '\n').count();

        // Move cursor up past previous render, then overwrite
        write!(stdout, "{}{}{}", up(last_lines), CLEAR_DOWN, block).ok();
        stdout.flush().ok();
        last_lines = line_count;

        if done {
            break;
        }

        tokio::time::sleep(refresh).await;
    }

    let _ = phase_handle.await;

    let sent = stats.sent.load(Ordering::Relaxed);
    let received = stats.received.load(Ordering::Relaxed);
    let errors = stats.errors.load(Ordering::Relaxed);
    let drop_pct = if sent > 0 {
        (1.0 - received as f64 / sent as f64).max(0.0) * 100.0
    } else {
        0.0
    };
    let achieved_rps = sent as f64 / duration.as_secs_f64();

    let mut lats = latencies.lock().await;
    lats.sort_unstable();
    let p50 = percentile(&lats, 50.0);
    let p95 = percentile(&lats, 95.0);
    let p99 = percentile(&lats, 99.0);

    PhaseResult {
        name: name.to_string(),
        target_rps,
        achieved_rps,
        sent,
        received,
        errors,
        drop_pct,
        p50_us: p50,
        p95_us: p95,
        p99_us: p99,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Dashboard Renderer — returns a fixed-height ANSI string
// ═══════════════════════════════════════════════════════════════════════════════

#[allow(clippy::too_many_arguments)]
fn render_phase_block(
    name: &str,
    phase_idx: usize,
    phase_total: usize,
    target_rps: u64,
    duration: Duration,
    elapsed: Duration,
    sent: u64,
    received: u64,
    errors: u64,
    drop_pct: f64,
    achieved_rps: f64,
    p50: u64,
    p95: u64,
    p99: u64,
    history: &[PhaseResult],
    domain_count: usize,
) -> String {
    let mut out = String::new();
    let pct = (elapsed.as_secs_f64() / duration.as_secs_f64() * 100.0).clamp(0.0, 100.0);
    let (verdict_str, verdict_color) = verdict(drop_pct);
    let drop_color = if drop_pct < 1.0 {
        GREEN
    } else if drop_pct < 5.0 {
        YELLOW
    } else {
        RED
    };

    // ── Phase header ─────────────────────────────────────────────────────
    out += &format!(
        " {BOLD}{CYAN}┌─ Phase {phase_idx}/{phase_total}  {WHITE}{name}{CYAN} ──────────────────────────────────────────────────┐{RST}\n"
    );

    // ── Progress bar ─────────────────────────────────────────────────────
    out += &format!(
        " {CYAN}│{RST}  {BOLD}Progress{RST}  {}  {DIM}{:>5.1}s / {:.0}s{RST}                     {CYAN}│{RST}\n",
        progress_bar(pct, 36),
        elapsed.as_secs_f64(),
        duration.as_secs_f64(),
    );

    // ── Sep ──────────────────────────────────────────────────────────────
    out += &format!(" {CYAN}│{DIM}  {}{RST}{CYAN}  │{RST}\n", "─".repeat(68));

    // ── Counters row 1 ───────────────────────────────────────────────────
    out += &format!(
        " {CYAN}│{RST}  {DIM}Sent{RST}      {WHITE}{:>13}{RST}   {DIM}Received{RST}  {WHITE}{:>13}{RST}   {DIM}Errors{RST} {RED}{:>6}{RST}  {CYAN}│{RST}\n",
        fmt_num(sent),
        fmt_num(received),
        fmt_num(errors),
    );

    // ── Counters row 2 ───────────────────────────────────────────────────
    out += &format!(
        " {CYAN}│{RST}  {DIM}Achieved{RST}  {WHITE}{:>10} rps{RST}   {DIM}Target{RST}    {YELLOW}{:>10} rps{RST}   {DIM}Drop{RST}  {drop_color}{:>7}{RST}  {CYAN}│{RST}\n",
        fmt_num(achieved_rps as u64),
        fmt_num(target_rps),
        fmt_pct(drop_pct),
    );

    // ── Sep ──────────────────────────────────────────────────────────────
    out += &format!(" {CYAN}│{DIM}  {}{RST}{CYAN}  │{RST}\n", "─".repeat(68));

    // ── Latency row ──────────────────────────────────────────────────────
    out += &format!(
        " {CYAN}│{RST}  {DIM}Latency{RST}   {GREEN}p50{RST} {WHITE}{}{RST}   {YELLOW}p95{RST} {WHITE}{}{RST}   {RED}p99{RST} {WHITE}{}{RST}                    {CYAN}│{RST}\n",
        fmt_ms(p50),
        fmt_ms(p95),
        fmt_ms(p99),
    );

    // ── Sep ──────────────────────────────────────────────────────────────
    out += &format!(" {CYAN}│{DIM}  {}{RST}{CYAN}  │{RST}\n", "─".repeat(68));

    // ── Traffic mix row ──────────────────────────────────────────────────
    out += &format!(
        " {CYAN}│{RST}  {DIM}Traffic{RST}   {GREEN}{}{RST} 75% Legit  {RED}{}{RST} 15% Blocked  {GREY}{}{RST} 10% NX            {CYAN}│{RST}\n",
        "█".repeat(15),
        "█".repeat(6),
        "█".repeat(4),
    );

    // ── Status footer ────────────────────────────────────────────────────
    out += &format!(
        " {BOLD}{CYAN}└─ {verdict_color}{verdict_str}{RST}{BOLD}{CYAN} ─── Domains loaded: {WHITE}{domain_count}{CYAN} ─── Sockets: {WHITE}{SOCKET_COUNT}{CYAN} ──────────────────┘{RST}\n"
    );

    // ── History section ──────────────────────────────────────────────────
    if !history.is_empty() {
        out += "\n";
        out += &format!(" {DIM}  Completed Phases{RST}\n");
        out += &format!(" {DIM}  {}{RST}\n", "─".repeat(70));
        for r in history {
            let (_, vc) = verdict(r.drop_pct);
            let drop_col = if r.drop_pct < 1.0 { GREEN } else { RED };
            out += &format!(
                " {DIM}  {vc}●{RST}  {:<22}  {:>9} → {WHITE}{:>9} rps{RST}   drop {drop_col}{}{RST}   p99 {WHITE}{}{RST}\n",
                r.name,
                fmt_num(r.target_rps),
                fmt_num(r.achieved_rps as u64),
                fmt_pct(r.drop_pct),
                fmt_ms(r.p99_us),
            );
        }
        out += &format!(" {DIM}  {}{RST}\n", "─".repeat(70));
    }

    out
}

// ═══════════════════════════════════════════════════════════════════════════════
// Binary Search — Find Max Sustainable RPS
// ═══════════════════════════════════════════════════════════════════════════════

const DROP_THRESHOLD: f64 = 1.0;
const SEARCH_DURATION: Duration = Duration::from_secs(10);

async fn binary_search_max_rps(addr: SocketAddr, legit_domains: Arc<Vec<String>>) -> u64 {
    let mut lo: u64 = 5_000;
    let mut hi: u64 = 300_000;
    let mut search_log: Vec<(u64, f64, f64, bool)> = Vec::new(); // (rps, drop, p99, sustainable)

    println!(
        "\n {BOLD}{MAGENTA}╔══════════════════════════════════════════════════════════════════════╗{RST}"
    );
    println!(
        " {BOLD}{MAGENTA}║{WHITE}      Binary Search — Finding Maximum Sustainable RPS             {MAGENTA}║{RST}"
    );
    println!(
        " {BOLD}{MAGENTA}╠══════════════════════════════════════════════════════════════════════╣{RST}"
    );
    println!(
        " {BOLD}{MAGENTA}║{RST}  Threshold : {GREEN}< {DROP_THRESHOLD:.0}% packet loss{RST}   Probe duration : {YELLOW}{:.0}s each{RST}             {MAGENTA}║{RST}",
        SEARCH_DURATION.as_secs_f64()
    );
    println!(
        " {BOLD}{MAGENTA}║{RST}  Range     : {WHITE}{} – {} RPS{RST}                                         {MAGENTA}║{RST}",
        fmt_num(lo),
        fmt_num(hi)
    );
    println!(
        " {BOLD}{MAGENTA}╚══════════════════════════════════════════════════════════════════════╝{RST}\n"
    );

    for step in 1..=10u32 {
        let mid = (lo + hi) / 2;

        print!(
            " {BOLD}  Step {:>2}/10{RST}  {YELLOW}{:>9} RPS{RST}  ",
            step,
            fmt_num(mid)
        );
        io::stdout().flush().ok();

        // Run a quick probe
        let stats = Arc::new(LiveStats::default());
        let lats: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));

        let cfg = PhaseRun {
            addr,
            target_rps: mid,
            duration: SEARCH_DURATION,
            legit_domains: legit_domains.clone(),
            stats: stats.clone(),
            latencies: lats.clone(),
        };
        run_phase(&cfg).await;

        let sent = stats.sent.load(Ordering::Relaxed);
        let received = stats.received.load(Ordering::Relaxed);
        let drop_pct = if sent > 0 {
            (1.0 - received as f64 / sent as f64).max(0.0) * 100.0
        } else {
            100.0
        };
        let mut lat_vec = lats.lock().await;
        lat_vec.sort_unstable();
        let p99 = percentile(&lat_vec, 99.0) as f64 / 1_000.0;

        let sustainable = drop_pct < DROP_THRESHOLD;
        search_log.push((mid, drop_pct, p99, sustainable));

        if sustainable {
            println!(
                "  drop {GREEN}{:>5.2}%{RST}   p99 {:>6.1}ms   {GREEN}✓ sustainable  → raise floor{RST}",
                drop_pct, p99
            );
            lo = mid;
        } else {
            println!(
                "  drop {RED}{:>5.2}%{RST}   p99 {:>6.1}ms   {RED}✗ dropping     → lower ceiling{RST}",
                drop_pct, p99
            );
            hi = mid;
        }

        if hi.saturating_sub(lo) < 2_000 {
            println!("\n  {DIM}Converged (Δ < 2,000 RPS){RST}");
            break;
        }
    }

    println!();
    lo
}

// ═══════════════════════════════════════════════════════════════════════════════
// Final Report
// ═══════════════════════════════════════════════════════════════════════════════

fn print_final_report(results: &[PhaseResult], max_rps: u64) {
    println!(
        " {BOLD}{WHITE}╔══════════════════════════════════════════════════════════════════════╗{RST}"
    );
    println!(
        " {BOLD}{WHITE}║                     BENCHMARK COMPLETE                              ║{RST}"
    );
    println!(
        " {BOLD}{WHITE}╠══════════════════════════════════════════════════════════════════════╣{RST}"
    );

    for r in results {
        let (vstr, vc) = verdict(r.drop_pct);
        println!(
            " {BOLD}{WHITE}║{RST}  {vc}{}{RST}  {:<20}  {:>8} rps   drop {:>6}   p99 {:<8} {WHITE}║{RST}",
            vstr,
            r.name,
            fmt_num(r.achieved_rps as u64),
            fmt_pct(r.drop_pct),
            fmt_ms(r.p99_us),
        );
    }

    println!(
        " {BOLD}{WHITE}╠══════════════════════════════════════════════════════════════════════╣{RST}"
    );
    println!(
        " {BOLD}{WHITE}║{RST}  {BOLD}{GREEN}Max Sustainable RPS  ≈  {:>12} rps{RST}                          {WHITE}║{RST}",
        fmt_num(max_rps)
    );
    println!(
        " {BOLD}{WHITE}║{RST}  {DIM}(sustained < {DROP_THRESHOLD:.0}% packet loss over {:.0}s probes){RST}                     {WHITE}║{RST}",
        SEARCH_DURATION.as_secs_f64()
    );
    println!(
        " {BOLD}{WHITE}╚══════════════════════════════════════════════════════════════════════╝{RST}\n"
    );

    if max_rps >= 280_000 {
        println!(
            " {YELLOW}Tip:{RST} Binary search hit the upper ceiling (300k RPS). rusthole can sustain"
        );
        println!(" even more — bump `hi` in `binary_search_max_rps()` and re-run.\n");
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Entry Point
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() {
    let addr_str = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:53".to_string());

    let addr = match SocketAddr::from_str(&addr_str) {
        Ok(a) => a,
        Err(_) => {
            eprintln!(
                "{RED}ERROR:{RST} '{}' is not a valid socket address.\n\
                 Usage: loadtest [ip:port]   e.g.  127.0.0.1:5353",
                addr_str
            );
            std::process::exit(1);
        }
    };

    // ── Banner ───────────────────────────────────────────────────────────
    print!("{HIDE_CURSOR}");
    println!(
        "\n {BOLD}{CYAN}╔══════════════════════════════════════════════════════════════════════╗{RST}"
    );
    println!(
        " {BOLD}{CYAN}║{WHITE}          rusthole DNS Sinkhole — Throughput Benchmark               {CYAN}║{RST}"
    );
    println!(
        " {BOLD}{CYAN}╠══════════════════════════════════════════════════════════════════════╣{RST}"
    );
    println!(
        " {BOLD}{CYAN}║{RST}  Target   : {WHITE}{:<52}{CYAN}║{RST}",
        format!("{}", addr)
    );
    println!(
        " {BOLD}{CYAN}║{RST}  Config   : {WHITE}{SOCKET_COUNT} sockets  ·  batch {BATCH_SIZE}  ·  latency sample 1/{SAMPLE_EVERY}{RST}          {CYAN}║{RST}"
    );
    println!(
        " {BOLD}{CYAN}║{RST}  Traffic  : {GREEN}75% Legit{RST} · {RED}15% Ad/Tracking (blocked){RST} · {GREY}10% NX{RST}         {CYAN}║{RST}"
    );
    println!(
        " {BOLD}{CYAN}╚══════════════════════════════════════════════════════════════════════╝{RST}\n"
    );

    // ── Fetch real domains ───────────────────────────────────────────────
    let (legit_raw, source) = fetch_legit_domains(8_000).await;
    println!(
        " {BOLD}{CYAN}  Domains  : {WHITE}{}{RST} from {CYAN}{source}{RST}\n",
        fmt_num(legit_raw.len() as u64)
    );
    let domain_count = legit_raw.len();
    let legit_domains = Arc::new(legit_raw);

    // ── Phase definitions ────────────────────────────────────────────────
    // Each tuple: (name, target_rps, duration_secs)
    let phases: &[(&str, u64, u64)] = &[
        ("Warm-up", 10_000, 10),
        ("Moderate Load", 50_000, 20),
        ("High Load", 100_000, 20),
        ("Stress Test", 150_000, 20),
    ];

    let phase_total = phases.len();
    let mut history: Vec<PhaseResult> = Vec::new();

    // ── Run phases ───────────────────────────────────────────────────────
    for (idx, &(name, target_rps, dur_secs)) in phases.iter().enumerate() {
        let result = execute_phase(
            name,
            addr,
            target_rps,
            Duration::from_secs(dur_secs),
            legit_domains.clone(),
            &history,
            idx + 1,
            phase_total,
            domain_count,
        )
        .await;
        history.push(result);
    }

    // ── Binary search ─────────────────────────────────────────────────────
    // Re-render history before starting search
    println!();
    let max_rps = binary_search_max_rps(addr, legit_domains.clone()).await;

    // ── Final report ──────────────────────────────────────────────────────
    print_final_report(&history, max_rps);
    print!("{SHOW_CURSOR}");
}
