use clap::Parser;
use iot_gateway::{ClientMessage, MessageType, create_client_config};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::time::{interval, sleep};
use tokio_rustls::TlsConnector;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Number of concurrent clients to spawn
    #[arg(short, long, default_value_t = 100)]
    clients: usize,

    /// Server address to connect to
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    server: String,

    /// Duration to run the test in seconds
    #[arg(short, long, default_value_t = 30)]
    duration: u64,

    /// Number of messages each client should send
    #[arg(short, long, default_value_t = 5)]
    messages: usize,

    /// Enable TLS connections
    #[arg(short, long, default_value_t = true)]
    tls: bool,

    /// Ramp-up time in seconds (gradually add clients)
    #[arg(short, long, default_value_t = 5)]
    rampup: u64,

    /// Maximum number of concurrent connections to allow
    #[arg(long, default_value_t = 1000)]
    max_concurrent: usize,
}

#[derive(Debug, Clone)]
struct TestStats {
    total_clients: usize,
    connected_clients: usize,
    failed_connections: usize,
    messages_sent: usize,
    messages_received: usize,
    total_bytes_sent: usize,
    total_bytes_received: usize,
    avg_connection_time_ms: f64,
    errors: usize,
}

impl TestStats {
    fn new() -> Self {
        Self {
            total_clients: 0,
            connected_clients: 0,
            failed_connections: 0,
            messages_sent: 0,
            messages_received: 0,
            total_bytes_sent: 0,
            total_bytes_received: 0,
            avg_connection_time_ms: 0.0,
            errors: 0,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    
    let args = Args::parse();
    
    info!("Starting IoT Gateway Performance Test");
    info!("Clients: {}, Duration: {}s, Messages per client: {}", 
          args.clients, args.duration, args.messages);
    info!("Server: {}, TLS: {}, Ramp-up: {}s", 
          args.server, args.tls, args.rampup);
    
    // Validate arguments
    if args.clients > 10000 {
        error!("Maximum number of clients is 10,000");
        return Ok(());
    }
    
    // Create shared statistics
    let stats = Arc::new(tokio::sync::RwLock::new(TestStats::new()));
    let stats_clone = stats.clone();
    
    // Start statistics reporting task
    tokio::spawn(async move {
        report_statistics(stats_clone, args.duration).await;
    });
    
    // Create semaphore for connection limiting
    let semaphore = Arc::new(Semaphore::new(args.max_concurrent));
    
    // Setup TLS if enabled
    let tls_connector = if args.tls {
        let client_config = create_client_config()?;
        Some(TlsConnector::from(Arc::new(client_config)))
    } else {
        None
    };
    
    let start_time = Instant::now();
    let mut client_handles = Vec::new();
    
    // Calculate client spawn interval for ramp-up
    let spawn_interval = if args.rampup > 0 && args.clients > 0 {
        Duration::from_millis((args.rampup * 1000) / args.clients as u64)
    } else {
        Duration::from_millis(0)
    };
    
    info!("Starting client spawn with {}ms intervals", spawn_interval.as_millis());
    
    for i in 0..args.clients {
        let server_addr = args.server.clone();
        let stats = stats.clone();
        let semaphore = semaphore.clone();
        let tls_connector = tls_connector.clone();
        let messages_to_send = args.messages;
        let test_duration = Duration::from_secs(args.duration);
        
        let handle = tokio::spawn(async move {
            run_client(
                i,
                server_addr,
                stats,
                semaphore,
                tls_connector,
                messages_to_send,
                test_duration,
                start_time,
            ).await;
        });
        
        client_handles.push(handle);
        
        // Gradual ramp-up
        if i < args.clients - 1 && spawn_interval.as_millis() > 0 {
            sleep(spawn_interval).await;
        }
    }
    
    info!("All {} clients spawned, waiting for test completion...", args.clients);
    
    // Wait for all clients to complete or timeout
    let timeout_duration = Duration::from_secs(args.duration + args.rampup + 30);
    match tokio::time::timeout(timeout_duration, futures_util::future::join_all(client_handles)).await {
        Ok(_) => info!("All clients completed successfully"),
        Err(_) => warn!("Test timed out, some clients may still be running"),
    }
    
    // Final statistics
    let final_stats = stats.read().await;
    print_final_statistics(&final_stats, start_time.elapsed());
    
    Ok(())
}

async fn run_client(
    client_id: usize,
    server_addr: String,
    stats: Arc<tokio::sync::RwLock<TestStats>>,
    semaphore: Arc<Semaphore>,
    tls_connector: Option<TlsConnector>,
    messages_to_send: usize,
    test_duration: Duration,
    test_start_time: Instant,
) {
    // Acquire semaphore permit for connection limiting
    let _permit = match semaphore.acquire().await {
        Ok(permit) => permit,
        Err(_) => {
            error!("Failed to acquire connection permit for client {}", client_id);
            return;
        }
    };
    
    let uuid = Uuid::new_v4();
    let connection_start = Instant::now();
    
    // Connect to server
    let stream = match TcpStream::connect(&server_addr).await {
        Ok(stream) => stream,
        Err(e) => {
            error!("Client {} failed to connect: {}", client_id, e);
            update_stats(&stats, |s| s.failed_connections += 1).await;
            return;
        }
    };
    
    let connection_time = connection_start.elapsed();
    
    // Update connection statistics
    update_stats(&stats, |s| {
        s.connected_clients += 1;
        s.total_clients += 1;
        s.avg_connection_time_ms = (s.avg_connection_time_ms * (s.total_clients - 1) as f64 + connection_time.as_millis() as f64) / s.total_clients as f64;
    }).await;
    
    // Handle TLS or plain connection
    let result = if let Some(connector) = tls_connector {
        match rustls::ServerName::try_from("localhost") {
            Ok(domain) => {
                match connector.connect(domain, stream).await {
                    Ok(tls_stream) => {
                        run_tls_client_session(client_id, uuid, tls_stream, &stats, messages_to_send, test_duration, test_start_time).await
                    }
                    Err(e) => {
                        error!("Client {} TLS handshake failed: {}", client_id, e);
                        update_stats(&stats, |s| s.failed_connections += 1).await;
                        return;
                    }
                }
            }
            Err(e) => {
                error!("Client {} invalid server name: {}", client_id, e);
                update_stats(&stats, |s| s.failed_connections += 1).await;
                return;
            }
        }
    } else {
        run_plain_client_session(client_id, uuid, stream, &stats, messages_to_send, test_duration, test_start_time).await
    };
    
    if let Err(e) = result {
        error!("Client {} session error: {}", client_id, e);
        update_stats(&stats, |s| s.errors += 1).await;
    }
    
    // Update disconnection statistics
    update_stats(&stats, |s| s.connected_clients -= 1).await;
}

async fn run_tls_client_session(
    _client_id: usize,
    uuid: Uuid,
    stream: tokio_rustls::client::TlsStream<TcpStream>,
    stats: &Arc<tokio::sync::RwLock<TestStats>>,
    messages_to_send: usize,
    test_duration: Duration,
    test_start_time: Instant,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    
    run_session_common(uuid, &mut reader, &mut writer, stats, messages_to_send, test_duration, test_start_time).await
}

async fn run_plain_client_session(
    _client_id: usize,
    uuid: Uuid,
    stream: TcpStream,
    stats: &Arc<tokio::sync::RwLock<TestStats>>,
    messages_to_send: usize,
    test_duration: Duration,
    test_start_time: Instant,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    
    run_session_common(uuid, &mut reader, &mut writer, stats, messages_to_send, test_duration, test_start_time).await
}

async fn run_session_common<R, W>(
    uuid: Uuid,
    reader: &mut BufReader<R>,
    writer: &mut W,
    stats: &Arc<tokio::sync::RwLock<TestStats>>,
    messages_to_send: usize,
    test_duration: Duration,
    test_start_time: Instant,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    R: tokio::io::AsyncRead + Unpin,
    W: AsyncWriteExt + Unpin,
{
    // Send connection message
    let connect_msg = ClientMessage {
        client_id: uuid,
        message_type: MessageType::Connect,
        payload: "Perf test client".to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    };
    
    let connect_json = serde_json::to_string(&connect_msg)? + "\n";
    writer.write_all(connect_json.as_bytes()).await?;
    update_stats(stats, |s| s.total_bytes_sent += connect_json.len()).await;
    
    // Read acknowledgment
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    update_stats(stats, |s| {
        s.messages_received += 1;
        s.total_bytes_received += line.len();
    }).await;
    
    // Track timing for keepalives and data messages
    let mut last_keepalive = Instant::now();
    let mut data_sent = 0;
    let mut last_data = Instant::now();
    let test_end_time = test_start_time + test_duration;
    
    // Send initial data messages and keepalives during the test
    loop {
        let now = Instant::now();
        
        // Check if test duration elapsed
        if now >= test_end_time {
            break;
        }
        
        // Send keepalive every 10 seconds
        if now.duration_since(last_keepalive) >= Duration::from_secs(10) {
            let keepalive_msg = ClientMessage {
                client_id: uuid,
                message_type: MessageType::Heartbeat,
                payload: "keepalive".to_string(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            };
            
            let keepalive_json = serde_json::to_string(&keepalive_msg)? + "\n";
            if writer.write_all(keepalive_json.as_bytes()).await.is_err() {
                break;
            }
            update_stats(stats, |s| {
                s.messages_sent += 1;
                s.total_bytes_sent += keepalive_json.len();
            }).await;
            
            last_keepalive = now;
        }
        
        // Send data message every 5 seconds (if we still have messages to send)
        if data_sent < messages_to_send && now.duration_since(last_data) >= Duration::from_secs(5) {
            let data_msg = ClientMessage {
                client_id: uuid,
                message_type: MessageType::Data,
                payload: format!("Perf test data #{}", data_sent + 1),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
            };
            
            let data_json = serde_json::to_string(&data_msg)? + "\n";
            if writer.write_all(data_json.as_bytes()).await.is_err() {
                break;
            }
            update_stats(stats, |s| {
                s.messages_sent += 1;
                s.total_bytes_sent += data_json.len();
            }).await;
            
            data_sent += 1;
            last_data = now;
        }
        
        // Small delay to prevent busy waiting
        sleep(Duration::from_millis(500)).await;
    }
    
    // Send disconnect message
    let disconnect_msg = ClientMessage {
        client_id: uuid,
        message_type: MessageType::Disconnect,
        payload: "Perf test complete".to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    };
    
    let disconnect_json = serde_json::to_string(&disconnect_msg)? + "\n";
    let _ = writer.write_all(disconnect_json.as_bytes()).await;
    update_stats(stats, |s| s.total_bytes_sent += disconnect_json.len()).await;
    
    Ok(())
}

async fn update_stats<F>(stats: &Arc<tokio::sync::RwLock<TestStats>>, update_fn: F) 
where
    F: FnOnce(&mut TestStats),
{
    let mut stats = stats.write().await;
    update_fn(&mut *stats);
}

async fn report_statistics(stats: Arc<tokio::sync::RwLock<TestStats>>, duration_secs: u64) {
    let mut interval = interval(Duration::from_secs(5));
    let start_time = Instant::now();
    
    loop {
        interval.tick().await;
        
        if start_time.elapsed().as_secs() >= duration_secs + 60 {
            break;
        }
        
        let current_stats = stats.read().await.clone();
        info!(
            "📊 Stats: Connected: {} | Failed: {} | Sent: {} msgs | Received: {} msgs | Errors: {} | Avg Conn: {:.1}ms",
            current_stats.connected_clients,
            current_stats.failed_connections,
            current_stats.messages_sent,
            current_stats.messages_received,
            current_stats.errors,
            current_stats.avg_connection_time_ms
        );
    }
}

fn print_final_statistics(stats: &TestStats, total_duration: Duration) {
    println!("\n🎯 PERFORMANCE TEST RESULTS");
    println!("═══════════════════════════════════════");
    println!("Total Duration: {:.2}s", total_duration.as_secs_f64());
    println!("Total Clients: {}", stats.total_clients);
    println!("Connected Successfully: {}", stats.total_clients - stats.failed_connections);
    println!("Failed Connections: {}", stats.failed_connections);
    
    if stats.total_clients > 0 {
        println!("Success Rate: {:.1}%", 
                 (stats.total_clients - stats.failed_connections) as f64 / stats.total_clients as f64 * 100.0);
    }
    
    println!("\n📡 MESSAGE STATISTICS");
    println!("Messages Sent: {}", stats.messages_sent);
    println!("Messages Received: {}", stats.messages_received);
    println!("Total Bytes Sent: {} KB", stats.total_bytes_sent / 1024);
    println!("Total Bytes Received: {} KB", stats.total_bytes_received / 1024);
    
    if total_duration.as_secs() > 0 {
        println!("Messages/sec: {:.1}", stats.messages_sent as f64 / total_duration.as_secs_f64());
        println!("Throughput: {:.1} KB/s", stats.total_bytes_sent as f64 / 1024.0 / total_duration.as_secs_f64());
    }
    
    println!("\n⚡ PERFORMANCE METRICS");
    println!("Avg Connection Time: {:.1}ms", stats.avg_connection_time_ms);
    println!("Errors: {}", stats.errors);
    
    if stats.total_clients > 0 {
        println!("Error Rate: {:.2}%", stats.errors as f64 / stats.total_clients as f64 * 100.0);
    }
    
    println!("═══════════════════════════════════════\n");
} 