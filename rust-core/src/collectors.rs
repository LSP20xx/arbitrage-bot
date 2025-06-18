use crate::common::{ArbitrageError, MempoolTransaction, Network, PoolReserves, PriceData};
use async_trait::async_trait;
use ethers::prelude::*;
use ethers::providers::{Middleware, Provider, Ws};
use ethers::types::{Address, BlockNumber, Filter, Log, Transaction, H256, U256};
use futures::stream::StreamExt;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Collector trait for gathering data from various sources
#[async_trait]
pub trait Collector: Send + Sync {
    /// Start the collector
    async fn start(&mut self) -> Result<(), ArbitrageError>;

    /// Stop the collector
    async fn stop(&mut self) -> Result<(), ArbitrageError>;

    /// Check if the collector is running
    fn is_running(&self) -> bool;

    /// Get the collector name
    fn name(&self) -> &str;

    /// Get the collector type
    fn collector_type(&self) -> CollectorType;
}

/// Collector types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectorType {
    Mempool,
    BlockEvents,
    PriceFeeds,
    PoolReserves,
    Custom,
}

/// Mempool collector configuration
#[derive(Debug, Clone)]
pub struct MempoolCollectorConfig {
    pub network: Network,
    pub ws_provider_url: String,
    pub filter_addresses: Option<Vec<Address>>,
    pub min_value: Option<U256>,
    pub max_transactions_per_second: Option<u32>,
    pub buffer_size: usize,
}

/// Mempool collector for monitoring pending transactions
pub struct MempoolCollector {
    config: MempoolCollectorConfig,
    provider: Option<Provider<Ws>>,
    tx_sender: Option<broadcast::Sender<MempoolTransaction>>,
    running: bool,
    name: String,
}

impl MempoolCollector {
    /// Create a new mempool collector
    pub fn new(config: MempoolCollectorConfig) -> Self {
        let name = format!("mempool_collector_{}", config.network);
        Self {
            config,
            provider: None,
            tx_sender: None,
            running: false,
            name,
        }
    }

    /// Get the transaction sender
    pub fn get_sender(&self) -> Option<broadcast::Sender<MempoolTransaction>> {
        self.tx_sender.clone()
    }

    /// Create a new transaction sender
    fn create_tx_sender(&self) -> broadcast::Sender<MempoolTransaction> {
        let (tx, _) = broadcast::channel(self.config.buffer_size);
        tx
    }

    /// Process a pending transaction
    async fn process_transaction(&self, tx: Transaction) -> Option<MempoolTransaction> {
        // Skip transactions with no recipient (contract deployments)
        let to = tx.to?;

        // Apply filters if configured
        if let Some(ref filter_addresses) = self.config.filter_addresses {
            if !filter_addresses.contains(&to) {
                return None;
            }
        }

        if let Some(min_value) = self.config.min_value {
            if tx.value < min_value {
                return None;
            }
        }

        // Convert to our internal transaction type
        let mempool_tx = MempoolTransaction {
            hash: tx.hash,
            from: tx.from,
            to: tx.to,
            value: tx.value,
            gas_price: tx.gas_price.unwrap_or_default(),
            gas_limit: tx.gas,
            nonce: tx.nonce.as_u64(),
            data: tx.input.to_vec(),
            timestamp: chrono::Utc::now(),
        };

        Some(mempool_tx)
    }
}

#[async_trait]
impl Collector for MempoolCollector {
    async fn start(&mut self) -> Result<(), ArbitrageError> {
        if self.running {
            return Ok(());
        }

        // Create WebSocket provider
        let ws = Ws::connect(&self.config.ws_provider_url)
            .await
            .map_err(|e| ArbitrageError::NetworkError(format!("Failed to connect to WebSocket: {}", e)))?;

        let provider = Provider::new(ws);
        self.provider = Some(provider.clone());

        // Create transaction sender
        let tx_sender = self.create_tx_sender();
        self.tx_sender = Some(tx_sender.clone());

        // Start watching for pending transactions
        let mut stream = provider
            .watch_pending_transactions()
            .await
            .map_err(|e| ArbitrageError::NetworkError(format!("Failed to watch pending transactions: {}", e)))?;

        self.running = true;
        info!("Started mempool collector for network: {}", self.config.network);

        // Spawn a task to process pending transactions
        let config = self.config.clone();
        let provider_clone = provider.clone();
        let network = config.network;

        tokio::spawn(async move {
            let rate_limit = config.max_transactions_per_second.map(|rate| {
                let duration = Duration::from_secs(1) / rate as u32;
                (rate, duration)
            });

            let mut last_processed = Instant::now();

            while let Some(tx_hash) = stream.next().await {
                // Apply rate limiting if configured
                if let Some((_, duration)) = rate_limit {
                    let elapsed = last_processed.elapsed();
                    if elapsed < duration {
                        tokio::time::sleep(duration - elapsed).await;
                    }
                    last_processed = Instant::now();
                }

                // Get full transaction details
                match provider_clone.get_transaction(tx_hash).await {
                    Ok(Some(tx)) => {
                        // Process the transaction
                        if let Some(mempool_tx) = self.process_transaction(tx).await {
                            // Send to subscribers
                            if let Err(e) = tx_sender.send(mempool_tx) {
                                warn!("Failed to send mempool transaction: {}", e);
                            }
                        }
                    }
                    Ok(None) => {
                        debug!("Transaction not found: {:?}", tx_hash);
                    }
                    Err(e) => {
                        error!("Error fetching transaction {}: {}", tx_hash, e);
                    }
                }
            }

            warn!("Mempool stream ended for network: {}", network);
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ArbitrageError> {
        if !self.running {
            return Ok(());
        }

        // Close the provider connection
        self.provider = None;
        self.tx_sender = None;
        self.running = false;

        info!("Stopped mempool collector for network: {}", self.config.network);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn collector_type(&self) -> CollectorType {
        CollectorType::Mempool
    }
}

/// Block events collector configuration
#[derive(Debug, Clone)]
pub struct BlockEventsCollectorConfig {
    pub network: Network,
    pub provider_url: String,
    pub start_block: Option<u64>,
    pub filter_addresses: Vec<Address>,
    pub filter_event_signatures: Vec<H256>,
    pub polling_interval_ms: u64,
    pub buffer_size: usize,
}

/// Block events collector for monitoring on-chain events
pub struct BlockEventsCollector {
    config: BlockEventsCollectorConfig,
    provider: Option<Arc<Provider<Http>>>,
    event_sender: Option<broadcast::Sender<Log>>,
    running: bool,
    name: String,
    current_block: u64,
}

impl BlockEventsCollector {
    /// Create a new block events collector
    pub fn new(config: BlockEventsCollectorConfig) -> Self {
        let name = format!("block_events_collector_{}", config.network);
        Self {
            config,
            provider: None,
            event_sender: None,
            running: false,
            name,
            current_block: 0,
        }
    }

    /// Get the event sender
    pub fn get_sender(&self) -> Option<broadcast::Sender<Log>> {
        self.event_sender.clone()
    }

    /// Create a new event sender
    fn create_event_sender(&self) -> broadcast::Sender<Log> {
        let (tx, _) = broadcast::channel(self.config.buffer_size);
        tx
    }

    /// Create an event filter
    fn create_filter(&self, from_block: u64, to_block: u64) -> Filter {
        let mut filter = Filter::new()
            .from_block(BlockNumber::Number(from_block.into()))
            .to_block(BlockNumber::Number(to_block.into()));

        // Add address filter if configured
        if !self.config.filter_addresses.is_empty() {
            filter = filter.address(self.config.filter_addresses.clone());
        }

        // Add event signature filters if configured
        if !self.config.filter_event_signatures.is_empty() {
            filter = filter.topics(
                Some(self.config.filter_event_signatures.clone()),
                None,
                None,
                None,
            );
        }

        filter
    }
}

#[async_trait]
impl Collector for BlockEventsCollector {
    async fn start(&mut self) -> Result<(), ArbitrageError> {
        if self.running {
            return Ok(());
        }

        // Create HTTP provider
        let provider = Arc::new(
            Provider::<Http>::try_from(&self.config.provider_url)
                .map_err(|e| ArbitrageError::NetworkError(format!("Failed to create HTTP provider: {}", e)))?,
        );
        self.provider = Some(provider.clone());

        // Create event sender
        let event_sender = self.create_event_sender();
        self.event_sender = Some(event_sender.clone());

        // Get current block number
        let current_block = provider
            .get_block_number()
            .await
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to get block number: {}", e)))?
            .as_u64();

        // Set starting block
        self.current_block = self.config.start_block.unwrap_or(current_block);

        self.running = true;
        info!(
            "Started block events collector for network: {} from block {}",
            self.config.network, self.current_block
        );

        // Spawn a task to poll for events
        let config = self.config.clone();
        let provider_clone = provider.clone();
        let network = config.network;
        let polling_interval = Duration::from_millis(config.polling_interval_ms);
        let mut current_block = self.current_block;

        tokio::spawn(async move {
            loop {
                // Sleep for polling interval
                tokio::time::sleep(polling_interval).await;

                // Get latest block number
                let latest_block = match provider_clone.get_block_number().await {
                    Ok(block) => block.as_u64(),
                    Err(e) => {
                        error!("Failed to get latest block number: {}", e);
                        continue;
                    }
                };

                // Skip if no new blocks
                if latest_block <= current_block {
                    continue;
                }

                // Process blocks in chunks to avoid large queries
                let chunk_size = 100;
                let mut from_block = current_block + 1;

                while from_block <= latest_block {
                    let to_block = std::cmp::min(from_block + chunk_size - 1, latest_block);
                    let filter = self.create_filter(from_block, to_block);

                    // Get logs for the filter
                    match provider_clone.get_logs(&filter).await {
                        Ok(logs) => {
                            for log in logs {
                                // Send to subscribers
                                if let Err(e) = event_sender.send(log) {
                                    warn!("Failed to send event log: {}", e);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to get logs for blocks {}-{}: {}", from_block, to_block, e);
                        }
                    }

                    from_block = to_block + 1;
                }

                // Update current block
                current_block = latest_block;
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ArbitrageError> {
        if !self.running {
            return Ok(());
        }

        // Close the provider connection
        self.provider = None;
        self.event_sender = None;
        self.running = false;

        info!("Stopped block events collector for network: {}", self.config.network);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn collector_type(&self) -> CollectorType {
        CollectorType::BlockEvents
    }
}

/// Pool reserves collector configuration
#[derive(Debug, Clone)]
pub struct PoolReservesCollectorConfig {
    pub network: Network,
    pub provider_url: String,
    pub pool_addresses: Vec<Address>,
    pub polling_interval_ms: u64,
    pub buffer_size: usize,
}

/// Pool reserves collector for monitoring DEX pool reserves
pub struct PoolReservesCollector {
    config: PoolReservesCollectorConfig,
    provider: Option<Arc<Provider<Http>>>,
    reserves_sender: Option<broadcast::Sender<PoolReserves>>,
    running: bool,
    name: String,
    // Cache of token addresses for each pool
    pool_tokens: HashMap<Address, (Address, Address)>,
}

impl PoolReservesCollector {
    /// Create a new pool reserves collector
    pub fn new(config: PoolReservesCollectorConfig) -> Self {
        let name = format!("pool_reserves_collector_{}", config.network);
        Self {
            config,
            provider: None,
            reserves_sender: None,
            running: false,
            name,
            pool_tokens: HashMap::new(),
        }
    }

    /// Get the reserves sender
    pub fn get_sender(&self) -> Option<broadcast::Sender<PoolReserves>> {
        self.reserves_sender.clone()
    }

    /// Create a new reserves sender
    fn create_reserves_sender(&self) -> broadcast::Sender<PoolReserves> {
        let (tx, _) = broadcast::channel(self.config.buffer_size);
        tx
    }

    /// Get token addresses for a pool
    async fn get_pool_tokens(&mut self, pool_address: Address, provider: Arc<Provider<Http>>) -> Result<(Address, Address), ArbitrageError> {
        // Check cache first
        if let Some((token0, token1)) = self.pool_tokens.get(&pool_address) {
            return Ok((*token0, *token1));
        }

        // Create contract instance for the pool
        // This is a simplified example - in a real implementation, you would use the correct ABI for the pool type
        let pool = Contract::new(
            pool_address,
            include_bytes!("../abis/UniswapV2Pair.json").to_vec(),
            provider.clone(),
        );

        // Get token0 and token1 addresses
        let token0: Address = pool
            .method::<_, Address>("token0", ())
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to create token0 method: {}", e)))?
            .call()
            .await
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to call token0: {}", e)))?;

        let token1: Address = pool
            .method::<_, Address>("token1", ())
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to create token1 method: {}", e)))?
            .call()
            .await
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to call token1: {}", e)))?;

        // Cache the result
        self.pool_tokens.insert(pool_address, (token0, token1));

        Ok((token0, token1))
    }

    /// Get reserves for a pool
    async fn get_pool_reserves(&mut self, pool_address: Address, provider: Arc<Provider<Http>>) -> Result<PoolReserves, ArbitrageError> {
        // Create contract instance for the pool
        let pool = Contract::new(
            pool_address,
            include_bytes!("../abis/UniswapV2Pair.json").to_vec(),
            provider.clone(),
        );

        // Get reserves
        let (reserve0, reserve1, _): (U256, U256, u32) = pool
            .method::<_, (U256, U256, u32)>("getReserves", ())
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to create getReserves method: {}", e)))?
            .call()
            .await
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to call getReserves: {}", e)))?;

        // Get token addresses
        let (token0, token1) = self.get_pool_tokens(pool_address, provider.clone()).await?;

        // Get current block number
        let block_number = provider
            .get_block_number()
            .await
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to get block number: {}", e)))?
            .as_u64();

        // Create pool reserves
        let reserves = PoolReserves {
            pool_address,
            token0,
            token1,
            reserve0,
            reserve1,
            block_number,
            timestamp: chrono::Utc::now(),
        };

        Ok(reserves)
    }
}

#[async_trait]
impl Collector for PoolReservesCollector {
    async fn start(&mut self) -> Result<(), ArbitrageError> {
        if self.running {
            return Ok(());
        }

        // Create HTTP provider
        let provider = Arc::new(
            Provider::<Http>::try_from(&self.config.provider_url)
                .map_err(|e| ArbitrageError::NetworkError(format!("Failed to create HTTP provider: {}", e)))?,
        );
        self.provider = Some(provider.clone());

        // Create reserves sender
        let reserves_sender = self.create_reserves_sender();
        self.reserves_sender = Some(reserves_sender.clone());

        self.running = true;
        info!(
            "Started pool reserves collector for network: {} with {} pools",
            self.config.network,
            self.config.pool_addresses.len()
        );

        // Spawn a task to poll for reserves
        let config = self.config.clone();
        let provider_clone = provider.clone();
        let network = config.network;
        let polling_interval = Duration::from_millis(config.polling_interval_ms);
        let pool_addresses = config.pool_addresses.clone();

        tokio::spawn(async move {
            let mut collector = PoolReservesCollector::new(config);

            loop {
                // Sleep for polling interval
                tokio::time::sleep(polling_interval).await;

                // Get reserves for each pool
                for pool_address in &pool_addresses {
                    match collector.get_pool_reserves(*pool_address, provider_clone.clone()).await {
                        Ok(reserves) => {
                            // Send to subscribers
                            if let Err(e) = reserves_sender.send(reserves) {
                                warn!("Failed to send pool reserves: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to get reserves for pool {}: {}", pool_address, e);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ArbitrageError> {
        if !self.running {
            return Ok(());
        }

        // Close the provider connection
        self.provider = None;
        self.reserves_sender = None;
        self.running = false;

        info!("Stopped pool reserves collector for network: {}", self.config.network);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn collector_type(&self) -> CollectorType {
        CollectorType::PoolReserves
    }
}

/// Price feed collector configuration
#[derive(Debug, Clone)]
pub struct PriceFeedCollectorConfig {
    pub network: Network,
    pub provider_url: String,
    pub token_addresses: Vec<Address>,
    pub price_feed_addresses: HashMap<Address, Address>, // Token address -> Price feed address
    pub polling_interval_ms: u64,
    pub buffer_size: usize,
}

/// Price feed collector for monitoring token prices
pub struct PriceFeedCollector {
    config: PriceFeedCollectorConfig,
    provider: Option<Arc<Provider<Http>>>,
    price_sender: Option<broadcast::Sender<PriceData>>,
    running: bool,
    name: String,
}

impl PriceFeedCollector {
    /// Create a new price feed collector
    pub fn new(config: PriceFeedCollectorConfig) -> Self {
        let name = format!("price_feed_collector_{}", config.network);
        Self {
            config,
            provider: None,
            price_sender: None,
            running: false,
            name,
        }
    }

    /// Get the price sender
    pub fn get_sender(&self) -> Option<broadcast::Sender<PriceData>> {
        self.price_sender.clone()
    }

    /// Create a new price sender
    fn create_price_sender(&self) -> broadcast::Sender<PriceData> {
        let (tx, _) = broadcast::channel(self.config.buffer_size);
        tx
    }

    /// Get price for a token
    async fn get_token_price(&self, token_address: Address, provider: Arc<Provider<Http>>) -> Result<PriceData, ArbitrageError> {
        // Get price feed address for the token
        let price_feed_address = self
            .config
            .price_feed_addresses
            .get(&token_address)
            .ok_or_else(|| ArbitrageError::ConfigurationError(format!("No price feed found for token {}", token_address)))?;

        // Create contract instance for the price feed
        // This is a simplified example - in a real implementation, you would use the correct ABI for the price feed
        let price_feed = Contract::new(
            *price_feed_address,
            include_bytes!("../abis/ChainlinkAggregator.json").to_vec(),
            provider.clone(),
        );

        // Get latest price
        let (round_id, price, _, timestamp, _): (U256, i128, U256, U256, U256) = price_feed
            .method::<_, (U256, i128, U256, U256, U256)>("latestRoundData", ())
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to create latestRoundData method: {}", e)))?
            .call()
            .await
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to call latestRoundData: {}", e)))?;

        // Get decimals
        let decimals: u8 = price_feed
            .method::<_, u8>("decimals", ())
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to create decimals method: {}", e)))?
            .call()
            .await
            .map_err(|e| ArbitrageError::BlockchainError(format!("Failed to call decimals: {}", e)))?;

        // Convert price to USD
        let price_usd = price as f64 / 10f64.powi(decimals as i32);

        // Create price data
        let price_data = PriceData {
            token: token_address,
            price_usd,
            timestamp: chrono::Utc::now(),
            source: format!("chainlink_{}", price_feed_address),
        };

        Ok(price_data)
    }
}

#[async_trait]
impl Collector for PriceFeedCollector {
    async fn start(&mut self) -> Result<(), ArbitrageError> {
        if self.running {
            return Ok(());
        }

        // Create HTTP provider
        let provider = Arc::new(
            Provider::<Http>::try_from(&self.config.provider_url)
                .map_err(|e| ArbitrageError::NetworkError(format!("Failed to create HTTP provider: {}", e)))?,
        );
        self.provider = Some(provider.clone());

        // Create price sender
        let price_sender = self.create_price_sender();
        self.price_sender = Some(price_sender.clone());

        self.running = true;
        info!(
            "Started price feed collector for network: {} with {} tokens",
            self.config.network,
            self.config.token_addresses.len()
        );

        // Spawn a task to poll for prices
        let config = self.config.clone();
        let provider_clone = provider.clone();
        let network = config.network;
        let polling_interval = Duration::from_millis(config.polling_interval_ms);
        let token_addresses = config.token_addresses.clone();

        tokio::spawn(async move {
            let collector = PriceFeedCollector::new(config);

            loop {
                // Sleep for polling interval
                tokio::time::sleep(polling_interval).await;

                // Get price for each token
                for token_address in &token_addresses {
                    match collector.get_token_price(*token_address, provider_clone.clone()).await {
                        Ok(price_data) => {
                            // Send to subscribers
                            if let Err(e) = price_sender.send(price_data) {
                                warn!("Failed to send price data: {}", e);
                            }
                        }
                        Err(e) => {
                            error!("Failed to get price for token {}: {}", token_address, e);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ArbitrageError> {
        if !self.running {
            return Ok(());
        }

        // Close the provider connection
        self.provider = None;
        self.price_sender = None;
        self.running = false;

        info!("Stopped price feed collector for network: {}", self.config.network);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn collector_type(&self) -> CollectorType {
        CollectorType::PriceFeeds
    }
}
