use crate::common::{
    ArbitrageError, ArbitrageOpportunity, DexInfo, DexType, Network, OpportunityStatus, PoolReserves,
    PriceData, StrategyConfig, StrategyType, TokenInfo,
};
use async_trait::async_trait;
use ethers::types::{Address, H256, U256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::broadcast;
use tokio::time::{Duration, Instant as TokioInstant};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Strategy trait for identifying arbitrage opportunities
#[async_trait]
pub trait Strategy: Send + Sync {
    /// Start the strategy
    async fn start(&mut self) -> Result<(), ArbitrageError>;

    /// Stop the strategy
    async fn stop(&mut self) -> Result<(), ArbitrageError>;

    /// Check if the strategy is running
    fn is_running(&self) -> bool;

    /// Get the strategy name
    fn name(&self) -> &str;

    /// Get the strategy type
    fn strategy_type(&self) -> StrategyType;

    /// Get the opportunity sender
    fn get_opportunity_sender(&self) -> Option<broadcast::Sender<ArbitrageOpportunity>>;
}

/// Base strategy configuration
#[derive(Debug, Clone)]
pub struct BaseStrategyConfig {
    pub strategy_config: StrategyConfig,
    pub dexes: HashMap<Address, DexInfo>,
    pub tokens: HashMap<Address, TokenInfo>,
    pub min_profit_threshold_usd: f64,
    pub max_gas_price_gwei: f64,
    pub buffer_size: usize,
    pub check_interval_ms: u64,
}

/// Base strategy implementation
pub struct BaseStrategy {
    config: BaseStrategyConfig,
    running: bool,
    name: String,
    opportunity_sender: Option<broadcast::Sender<ArbitrageOpportunity>>,
    // Cache of token prices
    token_prices: HashMap<Address, PriceData>,
    // Cache of pool reserves
    pool_reserves: HashMap<Address, PoolReserves>,
}

impl BaseStrategy {
    /// Create a new base strategy
    pub fn new(config: BaseStrategyConfig) -> Self {
        let name = config.strategy_config.name.clone();
        Self {
            config,
            running: false,
            name,
            opportunity_sender: None,
            token_prices: HashMap::new(),
            pool_reserves: HashMap::new(),
        }
    }

    /// Create a new opportunity sender
    fn create_opportunity_sender(&self) -> broadcast::Sender<ArbitrageOpportunity> {
        let (tx, _) = broadcast::channel(self.config.buffer_size);
        tx
    }

    /// Update token price
    pub fn update_token_price(&mut self, price_data: PriceData) {
        self.token_prices.insert(price_data.token, price_data);
    }

    /// Update pool reserves
    pub fn update_pool_reserves(&mut self, reserves: PoolReserves) {
        self.pool_reserves.insert(reserves.pool_address, reserves);
    }

    /// Get token price in USD
    pub fn get_token_price_usd(&self, token: &Address) -> Option<f64> {
        self.token_prices.get(token).map(|p| p.price_usd)
    }

    /// Convert token amount to USD
    pub fn token_amount_to_usd(&self, token: &Address, amount: U256) -> Option<f64> {
        let price = self.get_token_price_usd(token)?;
        let token_info = self.config.tokens.get(token)?;
        let decimals = token_info.decimals as u32;
        let amount_f64 = amount.as_u128() as f64 / 10f64.powi(decimals as i32);
        Some(amount_f64 * price)
    }

    /// Create a new arbitrage opportunity
    pub fn create_opportunity(
        &self,
        token_path: Vec<Address>,
        dex_path: Vec<Address>,
        expected_profit: U256,
        gas_cost: U256,
        network: Network,
        block_number: u64,
    ) -> ArbitrageOpportunity {
        ArbitrageOpportunity {
            id: Uuid::new_v4().to_string(),
            network,
            token_path,
            dex_path,
            expected_profit,
            gas_cost,
            timestamp: chrono::Utc::now(),
            block_number,
            status: OpportunityStatus::Pending,
            execution_tx: None,
        }
    }

    /// Send an arbitrage opportunity
    pub fn send_opportunity(&self, opportunity: ArbitrageOpportunity) -> Result<(), ArbitrageError> {
        if let Some(sender) = &self.opportunity_sender {
            sender.send(opportunity).map_err(|e| {
                ArbitrageError::InternalError(format!("Failed to send opportunity: {}", e))
            })?;
        }
        Ok(())
    }
}

#[async_trait]
impl Strategy for BaseStrategy {
    async fn start(&mut self) -> Result<(), ArbitrageError> {
        if self.running {
            return Ok(());
        }

        // Create opportunity sender
        let opportunity_sender = self.create_opportunity_sender();
        self.opportunity_sender = Some(opportunity_sender);

        self.running = true;
        info!("Started strategy: {}", self.name);

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ArbitrageError> {
        if !self.running {
            return Ok(());
        }

        self.opportunity_sender = None;
        self.running = false;

        info!("Stopped strategy: {}", self.name);
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn strategy_type(&self) -> StrategyType {
        self.config.strategy_config.strategy_type
    }

    fn get_opportunity_sender(&self) -> Option<broadcast::Sender<ArbitrageOpportunity>> {
        self.opportunity_sender.clone()
    }
}

/// Intra-chain DEX arbitrage strategy configuration
#[derive(Debug, Clone)]
pub struct IntraChainDexStrategyConfig {
    pub base_config: BaseStrategyConfig,
    pub max_path_length: usize,
    pub min_reserve_ratio: f64,
    pub max_price_impact: f64,
}

/// Intra-chain DEX arbitrage strategy
pub struct IntraChainDexStrategy {
    config: IntraChainDexStrategyConfig,
    base_strategy: BaseStrategy,
    // Cache of DEX pairs
    dex_pairs: HashMap<Address, Vec<(Address, Address)>>,
    // Cache of token paths
    token_paths: Vec<Vec<Address>>,
}

impl IntraChainDexStrategy {
    /// Create a new intra-chain DEX arbitrage strategy
    pub fn new(config: IntraChainDexStrategyConfig) -> Self {
        let base_strategy = BaseStrategy::new(config.base_config.clone());
        Self {
            config,
            base_strategy,
            dex_pairs: HashMap::new(),
            token_paths: Vec::new(),
        }
    }

    /// Initialize the strategy
    pub fn initialize(&mut self) -> Result<(), ArbitrageError> {
        // Build DEX pairs
        self.build_dex_pairs()?;

        // Build token paths
        self.build_token_paths()?;

        Ok(())
    }

    /// Build DEX pairs
    fn build_dex_pairs(&mut self) -> Result<(), ArbitrageError> {
        // Clear existing pairs
        self.dex_pairs.clear();

        // Get all DEXes from the configuration
        let dexes = &self.config.base_config.dexes;

        // For each DEX, get the pairs
        for (dex_address, dex_info) in dexes {
            let pairs = match dex_info.dex_type {
                DexType::UniswapV2 | DexType::SushiSwap => {
                    // For Uniswap V2-like DEXes, we need to query the factory for pairs
                    // This is a simplified example - in a real implementation, you would query the blockchain
                    vec![]
                }
                DexType::UniswapV3 => {
                    // For Uniswap V3, we need to query the factory for pools
                    // This is a simplified example - in a real implementation, you would query the blockchain
                    vec![]
                }
                DexType::Curve => {
                    // For Curve, we need to query the registry for pools
                    // This is a simplified example - in a real implementation, you would query the blockchain
                    vec![]
                }
                _ => vec![],
            };

            // Store the pairs
            self.dex_pairs.insert(*dex_address, pairs);
        }

        Ok(())
    }

    /// Build token paths
    fn build_token_paths(&mut self) -> Result<(), ArbitrageError> {
        // Clear existing paths
        self.token_paths.clear();

        // Get all tokens from the configuration
        let tokens = &self.config.base_config.strategy_config.tokens;

        // Build paths of length 2 (direct swaps)
        for &token_a in tokens {
            for &token_b in tokens {
                if token_a != token_b {
                    self.token_paths.push(vec![token_a, token_b]);
                }
            }
        }

        // Build paths of length 3 (triangular arbitrage)
        if self.config.max_path_length >= 3 {
            for &token_a in tokens {
                for &token_b in tokens {
                    for &token_c in tokens {
                        if token_a != token_b && token_b != token_c && token_c != token_a {
                            self.token_paths.push(vec![token_a, token_b, token_c, token_a]);
                        }
                    }
                }
            }
        }

        // Build longer paths if needed
        // This is a simplified example - in a real implementation, you would use a more sophisticated algorithm

        Ok(())
    }

    /// Check for arbitrage opportunities
    async fn check_opportunities(&self) -> Result<Vec<ArbitrageOpportunity>, ArbitrageError> {
        let mut opportunities = Vec::new();

        // Get the current block number
        // This is a simplified example - in a real implementation, you would get the actual block number
        let block_number = 0u64;

        // Check each token path
        for token_path in &self.token_paths {
            // Check if the path is a cycle (starts and ends with the same token)
            let is_cycle = token_path.first() == token_path.last();

            // Get the best DEX path for this token path
            if let Some((dex_path, profit)) = self.find_best_dex_path(token_path, block_number)? {
                // Check if the profit is above the threshold
                let profit_usd = self
                    .base_strategy
                    .token_amount_to_usd(token_path.first().unwrap(), profit)
                    .unwrap_or(0.0);

                if profit_usd >= self.config.base_config.min_profit_threshold_usd {
                    // Estimate the gas cost
                    let gas_cost = self.estimate_gas_cost(token_path, &dex_path)?;

                    // Create the opportunity
                    let opportunity = self.base_strategy.create_opportunity(
                        token_path.clone(),
                        dex_path,
                        profit,
                        gas_cost,
                        self.config.base_config.strategy_config.networks[0], // Simplified - use the first network
                        block_number,
                    );

                    // Add to the list of opportunities
                    opportunities.push(opportunity);
                }
            }
        }

        Ok(opportunities)
    }

    /// Find the best DEX path for a token path
    fn find_best_dex_path(
        &self,
        token_path: &[Address],
        block_number: u64,
    ) -> Result<Option<(Vec<Address>, U256)>, ArbitrageError> {
        // This is a simplified example - in a real implementation, you would use a more sophisticated algorithm
        // For example, you might use Dijkstra's algorithm to find the path with the highest profit

        // Get all DEXes from the configuration
        let dexes = &self.config.base_config.dexes;

        // Try each DEX for each hop in the path
        let mut best_profit = U256::zero();
        let mut best_dex_path = Vec::new();

        // For simplicity, we'll just try using the same DEX for all hops
        for (dex_address, dex_info) in dexes {
            let mut dex_path = Vec::new();
            let mut current_amount = U256::from(1_000_000_000_000_000_000u128); // 1 token with 18 decimals
            let mut success = true;

            // Simulate the swaps
            for i in 0..token_path.len() - 1 {
                let token_in = token_path[i];
                let token_out = token_path[i + 1];

                // Check if this DEX supports this pair
                if let Some(pairs) = self.dex_pairs.get(dex_address) {
                    if !pairs.contains(&(token_in, token_out)) && !pairs.contains(&(token_out, token_in)) {
                        success = false;
                        break;
                    }
                }

                // Simulate the swap
                match self.simulate_swap(dex_info, token_in, token_out, current_amount, block_number) {
                    Ok(amount_out) => {
                        current_amount = amount_out;
                        dex_path.push(*dex_address);
                    }
                    Err(_) => {
                        success = false;
                        break;
                    }
                }
            }

            // Check if the path was successful and profitable
            if success && current_amount > U256::from(1_000_000_000_000_000_000u128) {
                let profit = current_amount - U256::from(1_000_000_000_000_000_000u128);
                if profit > best_profit {
                    best_profit = profit;
                    best_dex_path = dex_path;
                }
            }
        }

        if best_profit > U256::zero() {
            Ok(Some((best_dex_path, best_profit)))
        } else {
            Ok(None)
        }
    }

    /// Simulate a swap on a DEX
    fn simulate_swap(
        &self,
        dex_info: &DexInfo,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        block_number: u64,
    ) -> Result<U256, ArbitrageError> {
        // This is a simplified example - in a real implementation, you would use the actual DEX formulas
        // For example, for Uniswap V2, you would use the constant product formula: x * y = k

        match dex_info.dex_type {
            DexType::UniswapV2 | DexType::SushiSwap => {
                // Find the pool for this pair
                let pool_address = self.find_pool(dex_info, token_in, token_out)?;

                // Get the reserves for this pool
                let reserves = self
                    .base_strategy
                    .pool_reserves
                    .get(&pool_address)
                    .ok_or_else(|| {
                        ArbitrageError::StrategyError(format!(
                            "No reserves found for pool {}",
                            pool_address
                        ))
                    })?;

                // Determine which token is token0 and which is token1
                let (reserve_in, reserve_out) = if token_in == reserves.token0 {
                    (reserves.reserve0, reserves.reserve1)
                } else {
                    (reserves.reserve1, reserves.reserve0)
                };

                // Calculate the output amount using the constant product formula
                // amount_out = (reserve_out * amount_in) / (reserve_in + amount_in)
                let amount_out = reserve_out
                    .checked_mul(amount_in)
                    .ok_or_else(|| ArbitrageError::StrategyError("Multiplication overflow".to_string()))?
                    .checked_div(reserve_in + amount_in)
                    .ok_or_else(|| ArbitrageError::StrategyError("Division by zero".to_string()))?;

                // Apply the fee (0.3% for Uniswap V2)
                let fee = amount_out
                    .checked_mul(U256::from(3))
                    .ok_or_else(|| ArbitrageError::StrategyError("Multiplication overflow".to_string()))?
                    .checked_div(U256::from(1000))
                    .ok_or_else(|| ArbitrageError::StrategyError("Division by zero".to_string()))?;

                let amount_out_after_fee = amount_out - fee;

                Ok(amount_out_after_fee)
            }
            DexType::UniswapV3 => {
                // Uniswap V3 uses a more complex formula with concentrated liquidity
                // This is a simplified example - in a real implementation, you would use the actual formula
                // For now, we'll just use a simplified version of the Uniswap V2 formula

                // Find the pool for this pair
                let pool_address = self.find_pool(dex_info, token_in, token_out)?;

                // Get the reserves for this pool
                let reserves = self
                    .base_strategy
                    .pool_reserves
                    .get(&pool_address)
                    .ok_or_else(|| {
                        ArbitrageError::StrategyError(format!(
                            "No reserves found for pool {}",
                            pool_address
                        ))
                    })?;

                // Determine which token is token0 and which is token1
                let (reserve_in, reserve_out) = if token_in == reserves.token0 {
                    (reserves.reserve0, reserves.reserve1)
                } else {
                    (reserves.reserve1, reserves.reserve0)
                };

                // Calculate the output amount using a simplified formula
                // amount_out = (reserve_out * amount_in) / (reserve_in + amount_in)
                let amount_out = reserve_out
                    .checked_mul(amount_in)
                    .ok_or_else(|| ArbitrageError::StrategyError("Multiplication overflow".to_string()))?
                    .checked_div(reserve_in + amount_in)
                    .ok_or_else(|| ArbitrageError::StrategyError("Division by zero".to_string()))?;

                // Apply the fee (0.3% for Uniswap V3 default pool)
                let fee = amount_out
                    .checked_mul(U256::from(3))
                    .ok_or_else(|| ArbitrageError::StrategyError("Multiplication overflow".to_string()))?
                    .checked_div(U256::from(1000))
                    .ok_or_else(|| ArbitrageError::StrategyError("Division by zero".to_string()))?;

                let amount_out_after_fee = amount_out - fee;

                Ok(amount_out_after_fee)
            }
            DexType::Curve => {
                // Curve uses a different formula for stable coins
                // This is a simplified example - in a real implementation, you would use the actual formula
                // For now, we'll just use a simplified version

                // Find the pool for this pair
                let pool_address = self.find_pool(dex_info, token_in, token_out)?;

                // Get the reserves for this pool
                let reserves = self
                    .base_strategy
                    .pool_reserves
                    .get(&pool_address)
                    .ok_or_else(|| {
                        ArbitrageError::StrategyError(format!(
                            "No reserves found for pool {}",
                            pool_address
                        ))
                    })?;

                // Determine which token is token0 and which is token1
                let (reserve_in, reserve_out) = if token_in == reserves.token0 {
                    (reserves.reserve0, reserves.reserve1)
                } else {
                    (reserves.reserve1, reserves.reserve0)
                };

                // Calculate the output amount using a simplified formula
                // For Curve, we'll use a formula that gives better rates for stable coins
                // amount_out = (reserve_out * amount_in) / (reserve_in + amount_in * 0.9)
                let amount_in_scaled = amount_in
                    .checked_mul(U256::from(9))
                    .ok_or_else(|| ArbitrageError::StrategyError("Multiplication overflow".to_string()))?
                    .checked_div(U256::from(10))
                    .ok_or_else(|| ArbitrageError::StrategyError("Division by zero".to_string()))?;

                let amount_out = reserve_out
                    .checked_mul(amount_in)
                    .ok_or_else(|| ArbitrageError::StrategyError("Multiplication overflow".to_string()))?
                    .checked_div(reserve_in + amount_in_scaled)
                    .ok_or_else(|| ArbitrageError::StrategyError("Division by zero".to_string()))?;

                // Apply the fee (0.04% for Curve)
                let fee = amount_out
                    .checked_mul(U256::from(4))
                    .ok_or_else(|| ArbitrageError::StrategyError("Multiplication overflow".to_string()))?
                    .checked_div(U256::from(10000))
                    .ok_or_else(|| ArbitrageError::StrategyError("Division by zero".to_string()))?;

                let amount_out_after_fee = amount_out - fee;

                Ok(amount_out_after_fee)
            }
            _ => Err(ArbitrageError::StrategyError(format!(
                "Unsupported DEX type: {:?}",
                dex_info.dex_type
            ))),
        }
    }

    /// Find a pool for a token pair
    fn find_pool(
        &self,
        dex_info: &DexInfo,
        token_a: Address,
        token_b: Address,
    ) -> Result<Address, ArbitrageError> {
        // This is a simplified example - in a real implementation, you would use the actual pool address
        // For example, for Uniswap V2, you would use the factory to compute the pool address

        // For now, we'll just return a dummy address
        Ok(Address::zero())
    }

    /// Estimate the gas cost for a transaction
    fn estimate_gas_cost(
        &self,
        token_path: &[Address],
        dex_path: &[Address],
    ) -> Result<U256, ArbitrageError> {
        // This is a simplified example - in a real implementation, you would estimate the gas cost based on the path
        // For example, you might use a gas estimator or a fixed gas cost per hop

        // For now, we'll just use a fixed gas cost
        let gas_limit = U256::from(500_000); // 500k gas
        let gas_price = U256::from(self.config.base_config.max_gas_price_gwei * 1e9); // Convert from Gwei to Wei

        Ok(gas_limit * gas_price)
    }
}

#[async_trait]
impl Strategy for IntraChainDexStrategy {
    async fn start(&mut self) -> Result<(), ArbitrageError> {
        if self.base_strategy.is_running() {
            return Ok(());
        }

        // Initialize the strategy
        self.initialize()?;

        // Start the base strategy
        self.base_strategy.start().await?;

        // Start the opportunity checking loop
        let config = self.config.clone();
        let base_config = config.base_config.clone();
        let strategy_name = self.base_strategy.name().to_string();
        let opportunity_sender = self.base_strategy.get_opportunity_sender().unwrap();
        let check_interval = Duration::from_millis(config.base_config.check_interval_ms);

        tokio::spawn(async move {
            let mut strategy = IntraChainDexStrategy::new(config);
            strategy.initialize().unwrap();

            let mut last_check = TokioInstant::now();

            loop {
                // Sleep until the next check
                let elapsed = last_check.elapsed();
                if elapsed < check_interval {
                    tokio::time::sleep(check_interval - elapsed).await;
                }
                last_check = TokioInstant::now();

                // Check for opportunities
                match strategy.check_opportunities().await {
                    Ok(opportunities) => {
                        for opportunity in opportunities {
                            if let Err(e) = opportunity_sender.send(opportunity) {
                                warn!("Failed to send opportunity: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to check opportunities: {}", e);
                    }
                }
            }
        });

        info!("Started intra-chain DEX strategy: {}", self.base_strategy.name());
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ArbitrageError> {
        // Stop the base strategy
        self.base_strategy.stop().await?;

        info!("Stopped intra-chain DEX strategy: {}", self.base_strategy.name());
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.base_strategy.is_running()
    }

    fn name(&self) -> &str {
        self.base_strategy.name()
    }

    fn strategy_type(&self) -> StrategyType {
        StrategyType::IntraChainDex
    }

    fn get_opportunity_sender(&self) -> Option<broadcast::Sender<ArbitrageOpportunity>> {
        self.base_strategy.get_opportunity_sender()
    }
}
