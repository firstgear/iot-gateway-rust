use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use rustls::{Certificate, PrivateKey, ServerConfig, ClientConfig};
use rustls_pemfile::{certs, pkcs8_private_keys};
use std::fs::File;
use std::io::BufReader;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    pub client_id: Uuid,
    pub message_type: MessageType,
    pub payload: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Connect,
    Disconnect,
    Data,
    Heartbeat,
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub id: Uuid,
    pub connected_at: std::time::Instant,
    pub last_heartbeat: std::time::Instant,
}

pub type ClientRegistry = Arc<RwLock<HashMap<Uuid, ClientInfo>>>;

pub fn create_client_registry() -> ClientRegistry {
    Arc::new(RwLock::new(HashMap::new()))
}

pub async fn register_client(registry: &ClientRegistry, client_id: Uuid) {
    let mut clients = registry.write().await;
    let now = std::time::Instant::now();
    clients.insert(client_id, ClientInfo {
        id: client_id,
        connected_at: now,
        last_heartbeat: now,
    });
}

pub async fn unregister_client(registry: &ClientRegistry, client_id: Uuid) {
    let mut clients = registry.write().await;
    clients.remove(&client_id);
}

pub async fn get_client_count(registry: &ClientRegistry) -> usize {
    let clients = registry.read().await;
    clients.len()
}

pub fn load_certs(path: &str) -> Result<Vec<Certificate>, Box<dyn std::error::Error>> {
    let certfile = File::open(path)?;
    let mut reader = BufReader::new(certfile);
    let certs = certs(&mut reader)?
        .into_iter()
        .map(Certificate)
        .collect();
    Ok(certs)
}

pub fn load_private_key(path: &str) -> Result<PrivateKey, Box<dyn std::error::Error>> {
    let keyfile = File::open(path)?;
    let mut reader = BufReader::new(keyfile);
    let keys = pkcs8_private_keys(&mut reader)?;
    
    if keys.is_empty() {
        return Err("No private key found".into());
    }
    
    Ok(PrivateKey(keys[0].clone()))
}

pub fn create_server_config(
    cert_path: &str,
    key_path: &str,
) -> Result<ServerConfig, Box<dyn std::error::Error>> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;
    
    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    
    Ok(config)
}

pub fn create_client_config() -> Result<ClientConfig, Box<dyn std::error::Error>> {
    let _root_cert_store = rustls::RootCertStore::empty();
    
    // For development with self-signed certificates, we'll skip certificate verification
    // In production, you would add proper CA certificates here
    let config = ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(Arc::new(NoVerifier))
        .with_no_client_auth();
    
    Ok(config)
}

// Custom certificate verifier that accepts all certificates (for development only)
struct NoVerifier;

impl rustls::client::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
} 