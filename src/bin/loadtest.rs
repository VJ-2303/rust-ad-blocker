//! src/bin/real_loadtest.rs
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use hdrhistogram::Histogram;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
};
use reqwest;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{net::UdpSocket, sync::Mutex};

// Configuration
const TARGET_ADDR: &str = "10.1.77.153:8053";
const TRANCO_LIST_URL: &str =
    "https://raw.githubusercontent.com/statscraft/top-1m/master/top-1m.csv";
const SOCKET_COUNT: usize = 8;
const DROP_THRESHOLD: f64 = 1.0; // 1% drop rate limit

#[derive(Default)]
struct Stats {
    sent: AtomicU64,
    received: AtomicU64,
    errors: AtomicU64,
    target_rps: AtomicU64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Fetching real-world domains from Tranco Top 1M list...");
    let domains = fetch_domains().await?;
    println!("Successfully loaded {} domains.", domains.len());

    println!("Pre-compiling DNS queries...");
    let packets = pre_build_packets(&domains);

    let stats = Arc::new(Stats::default());
    stats.target_rps.store(5_000, Ordering::Relaxed); // Start at 5k RPS

    let latencies = Arc::new(Mutex::new(Histogram::<u64>::new(3).unwrap()));

    // Setup TUI
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let target_addr: SocketAddr = TARGET_ADDR.parse()?;

    // Spawn network workers
    for idx in 0..SOCKET_COUNT {
        spawn_worker(
            idx,
            target_addr,
            stats.clone(),
            latencies.clone(),
            packets.clone(),
        );
    }

    // TUI Loop
    let start_time = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_sent = 0;
    let mut last_received = 0;

    loop {
        let current_sent = stats.sent.load(Ordering::Relaxed);
        let current_recv = stats.received.load(Ordering::Relaxed);
        let target_rps = stats.target_rps.load(Ordering::Relaxed);

        let dt = last_tick.elapsed().as_secs_f64();
        let rps_actual = (current_sent.saturating_sub(last_sent)) as f64 / dt;
        let drop_rate = if current_sent > last_sent {
            let dropped = (current_sent - last_sent).saturating_sub(current_recv - last_received);
            (dropped as f64 / (current_sent - last_sent) as f64) * 100.0
        } else {
            0.0
        };

        // Auto-Ramp Logic: Increase RPS by 2000 every second if drop rate is acceptable
        if dt >= 1.0 {
            if drop_rate < DROP_THRESHOLD {
                stats.target_rps.fetch_add(2000, Ordering::Relaxed);
            }
            last_tick = Instant::now();
            last_sent = current_sent;
            last_received = current_recv;
        }

        let lats = latencies.lock().await;
        let p50 = lats.value_at_percentile(50.0) as f64 / 1000.0;
        let p99 = lats.value_at_percentile(99.0) as f64 / 1000.0;
        drop(lats);

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Min(0),
                    ]
                    .as_ref(),
                )
                .split(f.size());

            let status_text = format!(
                " rusthole Real-World Load Tester | Target: {} | Uptime: {}s | Auto-Ramping: {}",
                TARGET_ADDR,
                start_time.elapsed().as_secs(),
                if drop_rate < DROP_THRESHOLD {
                    "ACTIVE 🟢"
                } else {
                    "CEILING REACHED 🔴"
                }
            );
            let p_status =
                Paragraph::new(status_text).block(Block::default().borders(Borders::ALL));
            f.render_widget(p_status, chunks[0]);

            let drop_color = if drop_rate < DROP_THRESHOLD {
                Color::Green
            } else {
                Color::Red
            };
            let gauge = Gauge::default()
                .block(
                    Block::default()
                        .title(" Packet Drop Rate ")
                        .borders(Borders::ALL),
                )
                .gauge_style(Style::default().fg(drop_color))
                .ratio((drop_rate / 10.0).clamp(0.0, 1.0))
                .label(format!("{:.2}%", drop_rate));
            f.render_widget(gauge, chunks[1]);

            let metrics = format!(
                "\n\n\
                Target RPS:      {:<10}\n\
                Actual RPS:      {:<10.0}\n\
                Packets Sent:    {:<10}\n\
                Packets Recv:    {:<10}\n\
                Errors (Socket): {:<10}\n\n\
                Latency p50:     {:.2} ms\n\
                Latency p99:     {:.2} ms\n\
                \n\nPress 'q' to quit.",
                target_rps,
                rps_actual,
                current_sent,
                current_recv,
                stats.errors.load(Ordering::Relaxed),
                p50,
                p99
            );
            let p_metrics = Paragraph::new(metrics).block(
                Block::default()
                    .title(" Live Metrics ")
                    .borders(Borders::ALL),
            );
            f.render_widget(p_metrics, chunks[2]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    // Cleanup TUI
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    println!(
        "Load test complete. Max achievable RPS before >1% drop: {}",
        stats.target_rps.load(Ordering::Relaxed)
    );
    Ok(())
}

async fn fetch_domains() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let text = reqwest::get(TRANCO_LIST_URL).await?.text().await?;
    let mut domains = Vec::new();
    // Parse CSV: "1,google.com"
    for line in text.lines().take(100_000) {
        // Limit to top 100k to save memory
        if let Some(domain) = line.split(',').nth(1) {
            domains.push(domain.to_string());
        }
    }
    Ok(domains)
}

fn pre_build_packets(domains: &[String]) -> Arc<Vec<Vec<u8>>> {
    let mut packets = Vec::with_capacity(domains.len());
    for domain in domains {
        let mut buf = Vec::with_capacity(64);
        buf.extend_from_slice(&[0x00, 0x00]); // ID (patched later)
        buf.extend_from_slice(&[0x01, 0x00]); // Flags: RD=1
        buf.extend_from_slice(&[0x00, 0x01]); // QDCOUNT = 1
        buf.extend_from_slice(&[0x00, 0x00]); // ANCOUNT
        buf.extend_from_slice(&[0x00, 0x00]); // NSCOUNT
        buf.extend_from_slice(&[0x00, 0x00]); // ARCOUNT
        for label in domain.split('.') {
            let b = label.as_bytes();
            buf.push(b.len() as u8);
            buf.extend_from_slice(b);
        }
        buf.push(0x00);
        buf.extend_from_slice(&[0x00, 0x01]); // Type A
        buf.extend_from_slice(&[0x00, 0x01]); // Class IN
        packets.push(buf);
    }
    Arc::new(packets)
}

fn spawn_worker(
    id: usize,
    target: SocketAddr,
    stats: Arc<Stats>,
    latencies: Arc<Mutex<Histogram<u64>>>,
    packets: Arc<Vec<Vec<u8>>>,
) {
    tokio::spawn(async move {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await.unwrap());
        socket.connect(target).await.unwrap();

        let pending: Arc<Mutex<HashMap<u16, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
        // Receiver Task
        let rx_socket = socket.clone();
        let rx_stats = stats.clone();
        let rx_pending = pending.clone();
        let rx_latencies = latencies.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            loop {
                if let Ok(len) = rx_socket.recv(&mut buf).await {
                    if len >= 2 {
                        rx_stats.received.fetch_add(1, Ordering::Relaxed);
                        let msg_id = u16::from_be_bytes([buf[0], buf[1]]);
                        if msg_id % 50 == 0 {
                            // Sample latency for 2% of packets
                            if let Some(start) = rx_pending.lock().await.remove(&msg_id) {
                                let _ = rx_latencies
                                    .lock()
                                    .await
                                    .record(start.elapsed().as_micros() as u64);
                            }
                        }
                    }
                }
            }
        });

        // Sender Task
        let mut packet_idx = id * 5000;
        let mut msg_id = id as u16 * 8000;

        loop {
            let target_rps = stats.target_rps.load(Ordering::Relaxed);
            let per_socket_rps = (target_rps / SOCKET_COUNT as u64).max(1);
            let delay = Duration::from_micros(1_000_000 / per_socket_rps);

            let start = Instant::now();

            let mut pkt = packets[packet_idx % packets.len()].clone();
            packet_idx = packet_idx.wrapping_add(1);
            msg_id = msg_id.wrapping_add(1);

            pkt[0] = (msg_id >> 8) as u8;
            pkt[1] = msg_id as u8;

            if msg_id % 50 == 0 {
                pending.lock().await.insert(msg_id, Instant::now());
            }

            if socket.send(&pkt).await.is_ok() {
                stats.sent.fetch_add(1, Ordering::Relaxed);
            } else {
                stats.errors.fetch_add(1, Ordering::Relaxed);
            }

            let elapsed = start.elapsed();
            if elapsed < delay {
                tokio::time::sleep(delay - elapsed).await;
            }
        }
    });
}
