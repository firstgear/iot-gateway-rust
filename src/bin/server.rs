use iot_gateway::{
    create_client_registry, get_client_count, register_client, unregister_client,
    ClientMessage, ClientRegistry, MessageType, create_server_config,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::interval;
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tracing::{error, info, warn};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    let bind_addr = "127.0.0.1:8080";
    let listener = TcpListener::bind(bind_addr).await?;
    
    info!("IoT Gateway Server starting on {} with TLS", bind_addr);
    
    // Setup TLS
    let server_config = create_server_config("certs/cert.pem", "certs/key.pem")?;
    let acceptor = TlsAcceptor::from(Arc::new(server_config));
    
    let client_registry = create_client_registry();
    
    // Start monitoring task
    let monitor_registry = client_registry.clone();
    tokio::spawn(async move {
        monitor_server_stats(monitor_registry).await;
    });
    
    // Accept connections
    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("New connection from {}", addr);
                let acceptor = acceptor.clone();
                let registry = client_registry.clone();
                tokio::spawn(async move {
                    match acceptor.accept(socket).await {
                        Ok(tls_stream) => {
                            if let Err(e) = handle_client(tls_stream, registry).await {
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
) -> Result<(), Box<dyn std::error::Error>> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    
    // Read the first message to get client ID
    reader.read_line(&mut line).await?;
    let message: ClientMessage = serde_json::from_str(line.trim())?;
    
    let client_id = message.client_id;
    info!("Client {} connected with TLS", client_id);
    
    // Register client
    register_client(&registry, client_id).await;
    
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

async fn monitor_server_stats(registry: ClientRegistry) {
    let mut interval = interval(Duration::from_secs(10));
    
    loop {
        interval.tick().await;
        
        let client_count = get_client_count(&registry).await;
        
        // Get memory usage
        let memory_mb = match get_memory_usage() {
            Ok(mem) => mem,
            Err(_) => 0.0,
        };
        
        info!(
            "=== Server Stats === Clients: {} | Memory: {:.1} MB | TLS Enabled",
            client_count, memory_mb
        );
    }
}

fn get_memory_usage() -> Result<f64, Box<dyn std::error::Error>> {
    let process = psutil::process::Process::current()?;
    let memory_info = process.memory_info()?;
    Ok(memory_info.rss() as f64 / 1024.0 / 1024.0) // Convert to MB
} 