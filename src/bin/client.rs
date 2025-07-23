use iot_gateway::{ClientMessage, MessageType, create_client_config};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::{interval, sleep};
use tokio_rustls::TlsConnector;
use tracing::{error, info};
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    let server_addr = "127.0.0.1:8080";
    let client_id = Uuid::new_v4();
    
    info!("IoT Client {} connecting to {} with TLS", client_id, server_addr);
    
    // Setup TLS
    let client_config = create_client_config()?;
    let connector = TlsConnector::from(Arc::new(client_config));
    
    // Connect to server
    let tcp_stream = TcpStream::connect(server_addr).await?;
    let domain = rustls::ServerName::try_from("localhost")?;
    let tls_stream = connector.connect(domain, tcp_stream).await?;
    
    let (reader, mut writer) = tokio::io::split(tls_stream);
    let mut reader = BufReader::new(reader);
    
    // Send initial connection message
    let connect_msg = ClientMessage {
        client_id,
        message_type: MessageType::Connect,
        payload: "Hello from TLS client".to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    };
    
    let connect_json = serde_json::to_string(&connect_msg)? + "\n";
    writer.write_all(connect_json.as_bytes()).await?;
    
    // Read server acknowledgment
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let ack: ClientMessage = serde_json::from_str(line.trim())?;
    info!("Server acknowledged TLS connection: {}", ack.payload);
    
    // Create a channel for sending messages to the writer
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ClientMessage>();
    
    // Start writer task
    let _writer_tx = tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(msg_json) = serde_json::to_string(&msg) {
                let msg_line = msg_json + "\n";
                if let Err(e) = writer.write_all(msg_line.as_bytes()).await {
                    error!("Failed to send message: {}", e);
                    break;
                }
            }
        }
    });
    
    // Start heartbeat task
    let heartbeat_tx = tx.clone();
    let heartbeat_client_id = client_id;
    tokio::spawn(async move {
        send_heartbeats(heartbeat_tx, heartbeat_client_id).await;
    });
    
    // Send some test data
    tokio::spawn(async move {
        send_test_data(tx, client_id).await;
    });
    
    // Keep the main task alive to read server messages
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                info!("Server closed connection");
                break;
            }
            Ok(_) => {
                match serde_json::from_str::<ClientMessage>(line.trim()) {
                    Ok(msg) => {
                        info!("Received from server: {:?}", msg);
                    }
                    Err(e) => {
                        error!("Failed to parse server message: {}", e);
                    }
                }
            }
            Err(e) => {
                error!("Error reading from server: {}", e);
                break;
            }
        }
    }
    
    Ok(())
}

async fn send_heartbeats(tx: tokio::sync::mpsc::UnboundedSender<ClientMessage>, client_id: Uuid) {
    let mut interval = interval(Duration::from_secs(10));
    
    loop {
        interval.tick().await;
        
        let heartbeat = ClientMessage {
            client_id,
            message_type: MessageType::Heartbeat,
            payload: "keepalive".to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        if tx.send(heartbeat).is_err() {
            error!("Failed to send heartbeat: channel closed");
            break;
        }
    }
}

async fn send_test_data(tx: tokio::sync::mpsc::UnboundedSender<ClientMessage>, client_id: Uuid) {
    sleep(Duration::from_secs(2)).await; // Wait a bit before sending data
    
    for i in 1..=5 {
        let data_msg = ClientMessage {
            client_id,
            message_type: MessageType::Data,
            payload: format!("Test data message #{}", i),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        if tx.send(data_msg).is_err() {
            error!("Failed to send data: channel closed");
            break;
        }
        info!("Sent test data #{}", i);
        
        sleep(Duration::from_secs(5)).await;
    }
    
    // Send disconnect message
    let disconnect_msg = ClientMessage {
        client_id,
        message_type: MessageType::Disconnect,
        payload: "Goodbye".to_string(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    
    if tx.send(disconnect_msg).is_err() {
        error!("Failed to send disconnect: channel closed");
    } else {
        info!("Sent disconnect message");
    }
} 