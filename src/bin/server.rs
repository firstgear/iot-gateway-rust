use iot_gateway::{
    create_client_registry, get_client_count, register_client, unregister_client,
    ClientMessage, ClientRegistry, MessageType, create_server_config,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::interval;
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug)]
pub struct ServerMetrics {
    pub start_time: Instant,
    pub total_connections: AtomicU64,
    pub total_messages: AtomicU64,
    pub total_bytes_received: AtomicU64,
    pub total_bytes_sent: AtomicU64,
    pub peak_clients: AtomicUsize,
    pub connections_last_second: AtomicU64,
    pub messages_last_second: AtomicU64,
    pub last_reset_time: Arc<std::sync::Mutex<Instant>>,
}

impl ServerMetrics {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            total_connections: AtomicU64::new(0),
            total_messages: AtomicU64::new(0),
            total_bytes_received: AtomicU64::new(0),
            total_bytes_sent: AtomicU64::new(0),
            peak_clients: AtomicUsize::new(0),
            connections_last_second: AtomicU64::new(0),
            messages_last_second: AtomicU64::new(0),
            last_reset_time: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }

    pub fn record_connection(&self) {
        self.total_connections.fetch_add(1, Ordering::Relaxed);
        self.connections_last_second.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_message(&self, bytes: usize) {
        self.total_messages.fetch_add(1, Ordering::Relaxed);
        self.total_bytes_received.fetch_add(bytes as u64, Ordering::Relaxed);
        self.messages_last_second.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_bytes_sent(&self, bytes: usize) {
        self.total_bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn update_peak_clients(&self, current_clients: usize) {
        let mut peak = self.peak_clients.load(Ordering::Relaxed);
        while current_clients > peak {
            match self.peak_clients.compare_exchange_weak(
                peak,
                current_clients,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => peak = x,
            }
        }
    }

    pub fn reset_per_second_counters(&self) {
        self.connections_last_second.store(0, Ordering::Relaxed);
        self.messages_last_second.store(0, Ordering::Relaxed);
        if let Ok(mut last_reset) = self.last_reset_time.lock() {
            *last_reset = Instant::now();
        }
    }

    pub fn get_uptime(&self) -> Duration {
        self.start_time.elapsed()
    }
}

pub type ServerMetricsRef = Arc<ServerMetrics>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    let bind_addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(bind_addr).await?;
    
    info!("🚀 IoT Gateway Server starting on {} with TLS", bind_addr);
    
    // Setup TLS
    let server_config = create_server_config("certs/cert.pem", "certs/key.pem")?;
    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    
    let client_registry = create_client_registry();
    let server_metrics = Arc::new(ServerMetrics::new());
    
    // Start monitoring task
    let monitor_registry = client_registry.clone();
    let monitor_metrics = server_metrics.clone();
    tokio::spawn(async move {
        monitor_server_stats(monitor_registry, monitor_metrics).await;
    });
    
    // Accept connections
    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("New connection from {}", addr);
                server_metrics.record_connection();
                
                let acceptor = acceptor.clone();
                let registry = client_registry.clone();
                let metrics = server_metrics.clone();
                tokio::spawn(async move {
                    match acceptor.accept(socket).await {
                        Ok(tls_stream) => {
                            if let Err(e) = handle_client(tls_stream, registry, metrics).await {
                                error!("Error handling client {}: {}", addr, e);
                            }
                        }
                        Err(e) => {
                            error!("TLS handshake failed for {}: {}", addr, e);
                        }
                    }
                });
            }
            Err(e) => {
                error!("Error accepting connection: {}", e);
            }
        }
    }
}

async fn handle_client(
    stream: TlsStream<TcpStream>,
    registry: ClientRegistry,
    metrics: ServerMetricsRef,
) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    
    // Read the first message to get client ID
    reader.read_line(&mut line).await?;
    metrics.record_message(line.len());
    let message: ClientMessage = serde_json::from_str(line.trim())?;
    
    let client_id = message.client_id;
    info!("Client {} connected with TLS", client_id);
    
    // Register client
    register_client(&registry, client_id).await;
    
    // Update peak clients
    let current_clients = get_client_count(&registry).await;
    metrics.update_peak_clients(current_clients);
    
    // Send acknowledgment
    let ack = ClientMessage {
        client_id: Uuid::new_v4(), // Server ID
        message_type: MessageType::Connect,
        payload: "Connected with TLS".to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    };
    let ack_json = serde_json::to_string(&ack)? + "\n";
    writer.write_all(ack_json.as_bytes()).await?;
    metrics.record_bytes_sent(ack_json.len());
    
    // Handle client messages
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                // Connection closed
                info!("Client {} disconnected", client_id);
                break;
            }
            Ok(_) => {
                metrics.record_message(line.len());
                match serde_json::from_str::<ClientMessage>(line.trim()) {
                    Ok(msg) => {
                        match msg.message_type {
                            MessageType::Disconnect => {
                                info!("Client {} requested disconnect", client_id);
                                break;
                            }
                            MessageType::Heartbeat => {
                                // Update heartbeat timestamp
                                // For now, just log it
                                info!("Heartbeat from client {}", client_id);
                            }
                            MessageType::Data => {
                                info!("Data from client {}: {}", client_id, msg.payload);
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        warn!("Invalid message from client {}: {}", client_id, e);
                    }
                }
            }
            Err(e) => {
                error!("Error reading from client {}: {}", client_id, e);
                break;
            }
        }
    }
    
    // Unregister client
    unregister_client(&registry, client_id).await;
    Ok(())
}

async fn monitor_server_stats(registry: ClientRegistry, metrics: ServerMetricsRef) {
    let mut interval = interval(Duration::from_secs(1));
    
    loop {
        interval.tick().await;
        
        let client_count = get_client_count(&registry).await;
        metrics.update_peak_clients(client_count);
        
        // Get memory usage
        let memory_mb = match get_memory_usage() {
            Ok(mem) => mem,
            Err(_) => 0.0,
        };
        
        // Get metrics
        let uptime = metrics.get_uptime();
        let total_connections = metrics.total_connections.load(Ordering::Relaxed);
        let total_messages = metrics.total_messages.load(Ordering::Relaxed);
        let total_bytes_rx = metrics.total_bytes_received.load(Ordering::Relaxed);
        let total_bytes_tx = metrics.total_bytes_sent.load(Ordering::Relaxed);
        let peak_clients = metrics.peak_clients.load(Ordering::Relaxed);
        let conn_rate = metrics.connections_last_second.load(Ordering::Relaxed);
        let msg_rate = metrics.messages_last_second.load(Ordering::Relaxed);
        
        // Calculate throughput (bytes per second)
        let uptime_secs = uptime.as_secs() as f64;
        let rx_throughput = if uptime_secs > 0.0 { total_bytes_rx as f64 / uptime_secs } else { 0.0 };
        let tx_throughput = if uptime_secs > 0.0 { total_bytes_tx as f64 / uptime_secs } else { 0.0 };
        
        // Format uptime
        let uptime_str = format_duration(uptime);
        
        info!(
            "📊 === ENHANCED SERVER STATS ===\n\
            🔗 Clients: {} (Peak: {}) | 📈 Conn Rate: {}/s | 💾 Memory: {:.1} MB\n\
            📨 Messages: {} ({}/s) | ⏱️ Uptime: {} | 🔐 TLS: Enabled\n\
            📡 RX: {:.1} KB ({:.1} B/s) | 📤 TX: {:.1} KB ({:.1} B/s) | 🔗 Total Conn: {}",
            client_count,
            peak_clients,
            conn_rate,
            memory_mb,
            total_messages,
            msg_rate,
            uptime_str,
            total_bytes_rx as f64 / 1024.0,
            rx_throughput,
            total_bytes_tx as f64 / 1024.0,
            tx_throughput,
            total_connections
        );
        
        // Reset per-second counters
        metrics.reset_per_second_counters();
    }
}

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    
    if hours > 0 {
        format!("{}h{}m{}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m{}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

fn get_memory_usage() -> Result<f64, Box<dyn std::error::Error>> {
    let process = psutil::process::Process::current()?;
    let memory_info = process.memory_info()?;
    Ok(memory_info.rss() as f64 / 1024.0 / 1024.0) // Convert to MB
} 