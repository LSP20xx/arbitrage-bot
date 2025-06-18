use crate::collectors::{Collector, OpportunityData};
use crate::common::{AppState, ArbitrageResult, Network};
use async_trait::async_trait;
use chrono::Utc;
use ethers::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

/// Price collector for fetching token prices
#[derive(Debug, Clone)]
pub struct PriceCollector {
    app_state: Arc<AppState>,
    network: Network,
    interval: Duration,
    running: bool,
    // Token addresses to monitor
    tokens: Vec<String>,
    // Base tokens for price calculation
    base_tokens: Vec<String>,
}

impl PriceCollector {
    /// Create a new price collector
    pub fn new(app_state: Arc<AppState>, network: Network, interval: Duration) -> Self {
        // Extract tokens to monitor from config
        let mut tokens = Vec::new();
        let mut base_tokens = Vec::new();

        // Add common tokens
        if let Some(weth_config) = app_state.config.tokens.get("weth") {
            if let Some(address) = weth_config.address_for_network(network) {
                tokens.push(address.clone());
                base_tokens.push(address);
            }
        }

        if let Some(usdc_config) = app_state.config.tokens.get("usdc") {
            if let Some(address) = usdc_config.address_for_network(network) {
                tokens.push(address.clone());
                base_tokens.push(address);
            }
        }

        // Add other tokens from config
        for (token_name, token_config) in &app_state.config.tokens {
            if token_name != "weth" && token_name != "usdc" {
                if let Some(address) = token_config.address_for_network(network) {
                    tokens.push(address);
                }
            }
        }

        Self {
            app_state,
            network,
            interval,
            running: false,
            tokens,
            base_tokens,
        }
    }

    /// Fetch price from Uniswap V2
    async fn fetch_price_uniswap_v2(
        &self,
        token_address: &str,
        base_token_address: &str,
    ) -> ArbitrageResult<Option<f64>> {
        // Get the provider for the network
        let providers = self.app_state.providers.read().unwrap();
        let provider = providers.get(&self.network).ok_or_else(|| {
            crate::common::ArbitrageError::Network(format!(
                "No provider found for network {}",
                self.network.name()
            ))
        })?;

        // Get the Uniswap V2 factory address
        let factory_address = self
            .app_state
            .config
            .dexes
            .get("uniswap_v2")
            .and_then(|dex| {
                if dex.enabled {
                    Some(dex.factory_address.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                crate::common::ArbitrageError::Config(
                    "Uniswap V2 factory address not found or disabled".to_string(),
                )
            })?;

        // Create a contract instance for the factory
        let factory = ethers::contract::Contract::new(
            factory_address.parse::<Address>()?,
            // Simplified ABI for Uniswap V2 factory
            serde_json::from_str::<ethers::abi::Abi>(
                r#"[{"inputs":[{"internalType":"address","name":"tokenA","type":"address"},{"internalType":"address","name":"tokenB","type":"address"}],"name":"getPair","outputs":[{"internalType":"address","name":"pair","type":"address"}],"stateMutability":"view","type":"function"}]"#,
            )?,
            provider.clone(),
        );

        // Get the pair address
        let pair_address: Address = factory
            .method::<_, Address>(
                "getPair",
                (
                    token_address.parse::<Address>()?,
                    base_token_address.parse::<Address>()?,
                ),
            )?
            .call()
            .await?;

        // If pair doesn't exist, return None
        if pair_address == Address::zero() {
            return Ok(None);
        }

        // Create a contract instance for the pair
        let pair = ethers::contract::Contract::new(
            pair_address,
            // Simplified ABI for Uniswap V2 pair
            serde_json::from_str::<ethers::abi::Abi>(
                r#"[{"inputs":[],"name":"getReserves","outputs":[{"internalType":"uint112","name":"_reserve0","type":"uint112"},{"internalType":"uint112","name":"_reserve1","type":"uint112"},{"internalType":"uint32","name":"_blockTimestampLast","type":"uint32"}],"stateMutability":"view","type":"function"},{"inputs":[],"name":"token0","outputs":[{"internalType":"address","name":"","type":"address"}],"stateMutability":"view","type":"function"},{"inputs":[],"name":"token1","outputs":[{"internalType":"address","name":"","type":"address"}],"stateMutability":"view","type":"function"}]"#,
            )?,
            provider.clone(),
        );

        // Get token0 and token1
        let token0: Address = pair.method::<_, Address>("token0", ())?.call().await?;
        let token1: Address = pair.method::<_, Address>("token1", ())?.call().await?;

        // Get reserves
        let (reserve0, reserve1, _): (U256, U256, u32) =
            pair.method::<_, (U256, U256, u32)>("getReserves", ())?.call().await?;

        // Calculate price based on token order
        let price = if token0 == token_address.parse::<Address>()? {
            // token0 is the token we want the price for
            reserve1.as_u128() as f64 / reserve0.as_u128() as f64
        } else {
            // token1 is the token we want the price for
            reserve0.as_u128() as f64 / reserve1.as_u128() as f64
        };

        Ok(Some(price))
    }

    /// Fetch prices for all tokens
    async fn fetch_prices(&self) -> ArbitrageResult<HashMap<String, f64>> {
        let mut prices = HashMap::new();

        // Fetch prices for each token using each base token
        for token in &self.tokens {
            for base_token in &self.base_tokens {
                if token == base_token {
                    continue;
                }

                match self.fetch_price_uniswap_v2(token, base_token).await {
                    Ok(Some(price)) => {
                        debug!(
                            "Fetched price for {}/{} on {}: {}",
                            token,
                            base_token,
                            self.network.name(),
                            price
                        );
                        prices.insert(format!("{}/{}", token, base_token), price);
                    }
                    Ok(None) => {
                        debug!(
                            "No pair found for {}/{} on {}",
                            token,
                            base_token,
                            self.network.name()
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Error fetching price for {}/{} on {}: {}",
                            token,
                            base_token,
                            self.network.name(),
                            e
                        );
                    }
                }
            }
        }

        Ok(prices)
    }

    /// Detect price opportunities
    fn detect_opportunities(&self, prices: &HashMap<String, f64>) -> Vec<OpportunityData> {
        let mut opportunities = Vec::new();

        // For now, just return the prices as opportunities
        // In a real implementation, we would analyze the prices to find arbitrage opportunities
        let timestamp = Utc::now().timestamp() as u64;
        let data = json!({
            "prices": prices,
        });

        opportunities.push(OpportunityData {
            network: self.network,
            source: "price".to_string(),
            timestamp,
            data,
        });

        opportunities
    }
}

#[async_trait]
impl Collector for PriceCollector {
    fn name(&self) -> &str {
        "PriceCollector"
    }

    fn network(&self) -> Network {
        self.network
    }

    async fn start(&mut self) -> ArbitrageResult<()> {
        if self.running {
            warn!("Price collector is already running");
            return Ok(());
        }

        info!("Starting price collector for network {}", self.network.name());
        self.running = true;
        Ok(())
    }

    async fn stop(&mut self) -> ArbitrageResult<()> {
        if !self.running {
            warn!("Price collector is not running");
            return Ok(());
        }

        info!("Stopping price collector for network {}", self.network.name());
        self.running = false;
        Ok(())
    }

    async fn collect(&self) -> ArbitrageResult<Vec<OpportunityData>> {
        if !self.running {
            return Ok(Vec::new());
        }

        // Fetch prices
        let prices = self.fetch_prices().await?;

        // Detect opportunities
        let opportunities = self.detect_opportunities(&prices);

        // Update price cache
        let mut price_cache = self.app_state.price_cache.lock().await;
        for (pair, price) in prices {
            price_cache.insert(pair, price);
        }

        // Sleep for the interval
        time::sleep(self.interval).await;

        Ok(opportunities)
    }
}
