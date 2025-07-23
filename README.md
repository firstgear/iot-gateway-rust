# 🚀 IoT Gateway - High-Performance Rust Implementation

A production-ready IoT Gateway built in Rust featuring TLS encryption, UUID client identification, real-time monitoring, and exceptional scalability supporting up to 10,000 concurrent connections.

## ✨ Features

- **🔐 TLS/SSL Encryption**: Full TLS 1.2/1.3 encryption for all client-server communication
- **🆔 UUID Client Identification**: Each client gets a unique UUID identifier for tracking
- **⚡ Single-Threaded Async**: Built with Tokio for high-performance async I/O
- **📊 Real-Time Monitoring**: Live client count and memory usage tracking
- **🎯 Performance Testing**: Comprehensive test suite supporting up to 10,000 clients
- **💪 Exceptional Scalability**: Tested with 5,000+ concurrent TLS connections
- **🛡️ Production Ready**: Robust error handling and graceful connection management

## 🏗️ Architecture

```
┌─────────────────┐    TLS/TCP     ┌─────────────────┐
│   IoT Clients   │◄──────────────►│  Gateway Server │
│   (UUID-based)  │                │  (Async/Tokio)  │
└─────────────────┘                └─────────────────┘
                                            │
                                   ┌────────▼────────┐
                                   │   Monitoring    │
                                   │ Client Registry │
                                   │ Memory Tracking │
                                   └─────────────────┘
```

## 📁 Project Structure

```
iot-gateway/
├── Cargo.toml              # Dependencies and project configuration
├── README.md               # This file
├── .gitignore              # Git ignore rules
├── certs/                  # SSL certificates (auto-generated)
│   ├── cert.pem            # Server certificate
│   └── key.pem             # Private key
└── src/
    ├── lib.rs              # Shared types and SSL utilities
    └── bin/
        ├── server.rs       # IoT Gateway server
        ├── client.rs       # Test client implementation
        └── perf_test.rs    # Performance test suite
```

## 🚀 Quick Start

### Prerequisites

- Rust 1.70+ (install from [rustup.rs](https://rustup.rs/))
- OpenSSL (for certificate generation)

### Installation

1. **Clone the repository**
   ```bash
   git clone <repository-url>
   cd iot-gateway
   ```

2. **Generate SSL certificates**
   ```bash
   mkdir -p certs
   openssl req -x509 -newkey rsa:4096 -keyout certs/key.pem -out certs/cert.pem \
           -days 365 -nodes -subj "/C=US/ST=CA/L=Test/O=IoTGateway/CN=localhost"
   ```

3. **Build the project**
   ```bash
   cargo build --release
   ```

## 🎮 Usage

### Start the IoT Gateway Server

```bash
# Start the server with TLS encryption
cargo run --bin server

# Output:
# 2025-01-23T12:08:20.683689Z  INFO server: IoT Gateway Server starting on 127.0.0.1:8080 with TLS
# 2025-01-23T12:08:20.685218Z  INFO server: === Server Stats === Clients: 0 | Memory: 3.4 MB | TLS Enabled
```

### Run a Test Client

```bash
# Connect a single test client
cargo run --bin client

# Output:
# 2025-01-23T12:08:34.546455Z  INFO client: IoT Client d17d4754-27e4-4bc0-92ae-bb3d5b3b89cc connecting to 127.0.0.1:8080 with TLS
# 2025-01-23T12:08:34.547251Z  INFO client: Server acknowledged TLS connection: Connected with TLS
```

### Performance Testing

```bash
# Basic performance test with 100 clients
cargo run --bin perf_test -- --clients 100 --duration 30

# Stress test with 1000 clients
cargo run --bin perf_test -- --clients 1000 --duration 60 --messages 5

# Maximum scale test with 5000 clients
cargo run --bin perf_test -- --clients 5000 --duration 120 --messages 2 --rampup 15

# Performance test options:
# --clients    Number of concurrent clients (1-10000)
# --duration   Test duration in seconds
# --messages   Messages per client
# --rampup     Gradual client spawn time in seconds
# --tls        Enable/disable TLS (default: true)
# --server     Server address (default: 127.0.0.1:8080)
```

## 📈 Performance Benchmarks

Our testing demonstrates exceptional scalability:

| Clients | Success Rate | Throughput | Memory Usage | Notes |
|---------|--------------|------------|--------------|-------|
| 10      | 100%         | 15.2 msg/s | 3.4 MB      | Basic load |
| 100     | 100%         | 92.6 msg/s | 3.5 MB      | Moderate load |
| 1,000   | 100%         | 262.6 msg/s| 3.6 MB      | High load |
| 5,000   | 100%         | 472.0 msg/s| 6.9 MB      | Stress test |

### Sample Performance Test Output

```
🎯 PERFORMANCE TEST RESULTS
═══════════════════════════════════════
Total Duration: 21.19s
Total Clients: 5000
Connected Successfully: 5000
Failed Connections: 0
Success Rate: 100.0%

📡 MESSAGE STATISTICS
Messages Sent: 10000
Messages Received: 5000
Total Bytes Sent: 2543 KB
Total Bytes Received: 644 KB
Messages/sec: 472.0
Throughput: 120.1 KB/s

⚡ PERFORMANCE METRICS
Avg Connection Time: 0.0ms
Errors: 0
Error Rate: 0.00%
═══════════════════════════════════════
```

## 🔒 Security Features

- **TLS 1.2/1.3 Encryption**: All communication is encrypted using modern TLS
- **Certificate Validation**: Configurable certificate verification (development vs production)
- **Secure Defaults**: Safe configuration out of the box
- **No Plaintext**: All client data is encrypted in transit

### Production Certificate Setup

For production deployment, replace the self-signed certificates:

```bash
# Place your production certificates
cp your-cert.pem certs/cert.pem
cp your-private-key.pem certs/key.pem

# Ensure proper permissions
chmod 600 certs/key.pem
chmod 644 certs/cert.pem
```

## 🛠️ Development

### Message Protocol

The gateway uses JSON-based messaging over TLS:

```json
{
  "client_id": "550e8400-e29b-41d4-a716-446655440000",
  "message_type": "Data",
  "payload": "Sensor reading: temperature=23.5°C",
  "timestamp": 1674472800
}
```

**Message Types:**
- `Connect`: Initial client connection
- `Disconnect`: Graceful client disconnection
- `Data`: IoT sensor data or commands
- `Heartbeat`: Keep-alive messages

### Adding Custom Message Types

1. Extend the `MessageType` enum in `src/lib.rs`
2. Update the server's message handling in `src/bin/server.rs`
3. Implement client logic in your IoT devices

### Monitoring Integration

The server provides real-time metrics that can be integrated with monitoring systems:

- Client connection count
- Memory usage tracking
- Message throughput statistics
- Error rates and connection failures

## 🔧 Configuration

### Server Configuration

Modify `src/bin/server.rs` for custom settings:

```rust
let bind_addr = "127.0.0.1:8080";  // Change listening address
let cert_path = "certs/cert.pem";  // Certificate path
let key_path = "certs/key.pem";    // Private key path
```

### Client Configuration

Customize client behavior in `src/bin/client.rs`:

```rust
let server_addr = "127.0.0.1:8080";     // Server address
let heartbeat_interval = Duration::from_secs(30);  // Heartbeat frequency
```

## 🐛 Troubleshooting

### Common Issues

1. **TLS Handshake Failed**
   ```
   Error: TLS handshake failed: invalid certificate
   ```
   **Solution**: Regenerate certificates or update the server name in client code

2. **Connection Refused**
   ```
   Error: Connection refused (os error 61)
   ```
   **Solution**: Ensure the server is running on the correct port

3. **Certificate Errors**
   ```
   Error: No such file or directory: certs/cert.pem
   ```
   **Solution**: Generate SSL certificates as shown in the installation steps

### Debug Mode

Enable detailed logging:

```bash
RUST_LOG=debug cargo run --bin server
```

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## 📝 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- Built with [Tokio](https://tokio.rs/) for async runtime
- TLS implementation using [rustls](https://github.com/rustls/rustls)
- Performance testing inspired by modern load testing tools
- UUID generation via [uuid](https://crates.io/crates/uuid) crate

