# DeFi Arbitrage Bot Platform Deployment Guide

This guide provides detailed instructions for deploying the DeFi Arbitrage Bot Platform in various environments.

## Prerequisites

Before deploying the platform, ensure you have the following prerequisites:

- **Rust**: Latest stable version (1.70+)
- **Node.js**: v16+ for smart contract development
- **Foundry**: For smart contract development and testing
- **RPC Access**: Access to RPC nodes for each network you want to monitor
  - Alchemy, Infura, or self-hosted nodes
  - WebSocket connections are required for mempool monitoring
- **Tenderly Account**: For transaction simulation (optional)
- **Private Keys**: For transaction signing (use a dedicated wallet for security)
- **Hardware Requirements**:
  - CPU: 4+ cores
  - RAM: 8GB+ (16GB+ recommended)
  - Storage: 100GB+ SSD
  - Network: Low-latency connection to Ethereum nodes

## Local Development Deployment

### 1. Clone the Repository

```bash
git clone https://github.com/yourusername/defi-arbitrage-bot.git
cd defi-arbitrage-bot
```

### 2. Install Dependencies

```bash
# Install Rust dependencies
cd packages/defi-arbitrage/rust-core
cargo build --release

# Install smart contract dependencies
cd ../contracts
forge install
```

### 3. Configure the Bot

Create a configuration file by copying the sample:

```bash
cp config.sample.json5 config.json5
```

Edit `config.json5` with your settings:

- Add your RPC URLs for each network
- Configure your wallet private key and address
- Set up Flashbots configuration
- Configure strategy parameters
- Add DEX addresses
- Add token addresses

### 4. Deploy Smart Contracts

Deploy the arbitrage executor contract to each network:

```bash
cd packages/defi-arbitrage/contracts
forge build
forge create --rpc-url <RPC_URL> --private-key <PRIVATE_KEY> src/arbitrage/ArbitrageExecutor.sol:ArbitrageExecutor
```

Update your `config.json5` with the deployed contract addresses.

### 5. Run the Bot

```bash
cd packages/defi-arbitrage/rust-core
cargo run --release -- --config ../../config.json5
```

## Production Deployment

For production deployment, it's recommended to use a dedicated server with low latency to Ethereum nodes.

### 1. Server Setup

Set up a server with the following specifications:

- Ubuntu 22.04 LTS or similar
- 4+ CPU cores
- 16GB+ RAM
- 100GB+ SSD storage
- Low-latency connection to Ethereum nodes

### 2. Install Dependencies

```bash
# Update system packages
sudo apt update
sudo apt upgrade -y

# Install build tools
sudo apt install -y build-essential pkg-config libssl-dev

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# Install Node.js
curl -fsSL https://deb.nodesource.com/setup_18.x | sudo -E bash -
sudo apt install -y nodejs

# Install Foundry
curl -L https://foundry.paradigm.xyz | bash
foundryup
```

### 3. Clone and Build the Repository

```bash
git clone https://github.com/yourusername/defi-arbitrage-bot.git
cd defi-arbitrage-bot

# Build Rust components
cd packages/defi-arbitrage/rust-core
cargo build --release

# Build smart contracts
cd ../contracts
forge build
```

### 4. Configure the Bot

Create a configuration file:

```bash
cp config.sample.json5 config.json5
```

Edit `config.json5` with your production settings.

### 5. Deploy Smart Contracts

Deploy the arbitrage executor contract to each network:

```bash
cd packages/defi-arbitrage/contracts
forge create --rpc-url <RPC_URL> --private-key <PRIVATE_KEY> src/arbitrage/ArbitrageExecutor.sol:ArbitrageExecutor
```

Update your `config.json5` with the deployed contract addresses.

### 6. Set Up Systemd Service

Create a systemd service file for automatic startup and management:

```bash
sudo nano /etc/systemd/system/arbitrage-bot.service
```

Add the following content:

```
[Unit]
Description=DeFi Arbitrage Bot
After=network.target

[Service]
User=<your-username>
WorkingDirectory=/path/to/defi-arbitrage-bot/packages/defi-arbitrage/rust-core
ExecStart=/path/to/defi-arbitrage-bot/packages/defi-arbitrage/rust-core/target/release/defi-arbitrage-bot --config ../../config.json5
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
```

Enable and start the service:

```bash
sudo systemctl enable arbitrage-bot
sudo systemctl start arbitrage-bot
```

### 7. Monitor the Bot

Check the status of the bot:

```bash
sudo systemctl status arbitrage-bot
```

View logs:

```bash
sudo journalctl -u arbitrage-bot -f
```

## Docker Deployment

### 1. Build the Docker Image

```bash
cd defi-arbitrage-bot
docker build -t defi-arbitrage-bot .
```

### 2. Create a Configuration File

Create a `config.json5` file with your settings.

### 3. Run the Docker Container

```bash
docker run -d \
  --name arbitrage-bot \
  -v $(pwd)/config.json5:/app/config.json5 \
  -v $(pwd)/data:/app/data \
  defi-arbitrage-bot
```

### 4. Monitor the Container

```bash
docker logs -f arbitrage-bot
```

## Cloud Deployment (AWS)

### 1. Launch an EC2 Instance

- AMI: Ubuntu Server 22.04 LTS
- Instance Type: c5.xlarge or better (4 vCPUs, 8GB RAM)
- Storage: 100GB+ gp3 SSD
- Security Group: Allow SSH access

### 2. Connect to the Instance

```bash
ssh -i your-key.pem ubuntu@your-instance-ip
```

### 3. Install Dependencies

Follow the same steps as in the Production Deployment section.

### 4. Configure and Run the Bot

Follow the same steps as in the Production Deployment section.

### 5. Set Up CloudWatch Monitoring

Create a CloudWatch dashboard to monitor the bot:

```bash
# Install CloudWatch agent
sudo apt install -y amazon-cloudwatch-agent

# Configure CloudWatch agent
sudo nano /opt/aws/amazon-cloudwatch-agent/etc/amazon-cloudwatch-agent.json
```

Add configuration for monitoring CPU, memory, disk, and network usage.

Start the CloudWatch agent:

```bash
sudo systemctl enable amazon-cloudwatch-agent
sudo systemctl start amazon-cloudwatch-agent
```

## Security Considerations

### Private Key Management

For production deployments, it's crucial to secure your private keys:

1. **Use Environment Variables**: Store private keys in environment variables rather than in configuration files.

   ```bash
   export ARBITRAGE_BOT_PRIVATE_KEY=your-private-key
   ```

   Then modify your code to read from environment variables.

2. **Use a Hardware Security Module (HSM)**: For high-value deployments, consider using an HSM like YubiHSM or AWS CloudHSM.

3. **Use a Key Management Service**: Consider using AWS KMS, Google Cloud KMS, or HashiCorp Vault for key management.

### Network Security

1. **Firewall Configuration**: Restrict inbound connections to only necessary ports.

2. **VPN Access**: Use a VPN for accessing your server.

3. **DDoS Protection**: Consider using a DDoS protection service like Cloudflare or AWS Shield.

### Monitoring and Alerts

1. **Set Up Monitoring**: Use Prometheus and Grafana for monitoring.

2. **Configure Alerts**: Set up alerts for critical events like:
   - High gas prices
   - Failed transactions
   - Circuit breaker activation
   - Server resource exhaustion

## Performance Optimization

### RPC Node Selection

Choose RPC nodes with low latency and high reliability:

1. **Self-hosted Nodes**: For best performance, run your own Ethereum nodes.

2. **Premium RPC Services**: Use premium RPC services like Alchemy or Infura.

3. **Geographic Proximity**: Choose nodes that are geographically close to your server.

### Gas Price Optimization

Configure gas price strategies for optimal execution:

1. **Dynamic Gas Pricing**: Use dynamic gas pricing based on network conditions.

2. **Gas Price Limits**: Set maximum gas prices to avoid overpaying.

3. **Priority Fee Optimization**: Optimize priority fees for faster inclusion.

### Capital Efficiency

Optimize capital usage for maximum returns:

1. **Flash Loans**: Use flash loans for capital-efficient arbitrage.

2. **Capital Pre-positioning**: Pre-position capital on different chains for cross-chain arbitrage.

3. **Multi-hop Strategies**: Implement multi-hop strategies for better capital efficiency.

## Troubleshooting

### Common Issues

1. **RPC Connection Issues**:
   - Check your RPC URLs
   - Verify network connectivity
   - Check if the RPC provider is operational

2. **Transaction Failures**:
   - Check gas prices
   - Verify contract approvals
   - Check for slippage issues

3. **Performance Issues**:
   - Monitor CPU and memory usage
   - Check for network latency
   - Optimize strategy parameters

### Logs and Diagnostics

1. **Log Levels**: Adjust log levels for more detailed information:
   ```bash
   RUST_LOG=debug cargo run --release -- --config config.json5
   ```

2. **Metrics**: Monitor metrics for performance analysis.

3. **Profiling**: Use profiling tools to identify bottlenecks.

## Conclusion

This deployment guide provides a comprehensive overview of deploying the DeFi Arbitrage Bot Platform in various environments. By following these instructions, you can set up a robust and secure arbitrage bot for production use.

Remember to always test thoroughly in a development environment before deploying to production, and start with small amounts of capital to validate your strategies before scaling up.
