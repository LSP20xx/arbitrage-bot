use crate::collectors::{Collector, OpportunityData};
use crate::common::{AppState, ArbitrageResult, Network};
use async_trait::async_trait;
use chrono::Utc;
use ethers::prelude::*;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

/// Mempool collector for monitoring pending transactions
#[derive(Debug, Clone)]
pub struct MempoolCollector {
    app_state: Arc<AppState>,
    network: Network,
    interval: Duration,
    running: bool,
    // DEX router addresses to monitor
    dex_routers: HashSet<Address>,
    // Last processed block number
    last_block: Option<u64>,
}

impl MempoolCollector {
    /// Create a new mempool collector
    pub fn new(app_state: Arc<AppState>, network: Network, interval: Duration) -> Self {
        // Extract DEX router addresses to monitor
        let mut dex_routers = HashSet::new();

        // Add Uniswap V2 router
        if let Some(dex_config) = app_state.config.dexes.get("uniswap_v2") {
            if dex_config.enabled {
                if let Some(router_address) = &dex_config.router_address {
                    if let Ok(address) = router_address.parse::<Address>() {
                        dex_routers.insert(address);
                    }
                }
            }
        }

        // Add Uniswap V3 router
        if let Some(dex_config) = app_state.config.dexes.get("uniswap_v3") {
            if dex_config.enabled {
                if let Some(router_address) = &dex_config.router_address {
                    if let Ok(address) = router_address.parse::<Address>() {
                        dex_routers.insert(address);
                    }
                }
            }
        }

        // Add Sushiswap router
        if let Some(dex_config) = app_state.config.dexes.get("sushiswap") {
            if dex_config.enabled {
                if let Some(router_address) = &dex_config.router_address {
                    if let Ok(address) = router_address.parse::<Address>() {
                        dex_routers.insert(address);
                    }
                }
            }
        }

        Self {
            app_state,
            network,
            interval,
            running: false,
            dex_routers,
            last_block: None,
        }
    }

    /// Monitor pending transactions
    async fn monitor_pending_transactions(&self) -> ArbitrageResult<Vec<OpportunityData>> {
        // Get the provider for the network
        let providers = self.app_state.providers.read().unwrap();
        let provider = providers.get(&self.network).ok_or_else(|| {
            crate::common::ArbitrageError::Network(format!(
                "No provider found for network {}",
                self.network.name()
            ))
        })?;

        // Get the current block number
        let current_block = provider.get_block_number().await?;
        let from_block = self.last_block.unwrap_or(current_block.as_u64().saturating_sub(10));

        debug!(
            "Monitoring pending transactions from block {} to {}",
            from_block,
            current_block
        );

        // Get pending transactions
        let pending_txs = provider.get_block(BlockNumber::Pending).await?;

        let mut opportunities = Vec::new();

        if let Some(block) = pending_txs {
            if let Some(txs) = block.transactions {
                for tx_hash in txs {
                    // Get the transaction details
                    if let Ok(Some(tx)) = provider.get_transaction(tx_hash).await {
                        // Check if the transaction is to a DEX router
                        if let Some(to) = tx.to {
                            if self.dex_routers.contains(&to) {
                                debug!("Found DEX transaction: {:?}", tx_hash);

                                // Create an opportunity
                                let timestamp = Utc::now().timestamp() as u64;
                                let data = json!({
                                    "tx_hash": tx_hash.to_string(),
                                    "from": tx.from.to_string(),
                                    "to": to.to_string(),
                                    "value": tx.value.to_string(),
                                    "gas_price": tx.gas_price.map(|gp| gp.to_string()),
                                    "input": format!("0x{}", hex::encode(&tx.input)),
                                });

                                opportunities.push(OpportunityData {
                                    network: self.network,
                                    source: "mempool".to_string(),
                                    timestamp,
                                    data,
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(opportunities)
    }

    /// Analyze transaction input data to detect swaps
    fn analyze_transaction_input(&self, input: &[u8]) -> Option<(String, String, String)> {
        // This is a simplified implementation
        // In a real-world scenario, we would decode the transaction input
        // to extract the exact swap parameters (token addresses, amounts, etc.)

        // Check for Uniswap V2 swapExactTokensForTokens function signature
        if input.len() >= 4 && &input[0..4] == &[0x38, 0xed, 0x17, 0x39] {
            return Some((
                "swapExactTokensForTokens".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
            ));
        }

        // Check for Uniswap V2 swapTokensForExactTokens function signature
        if input.len() >= 4 && &input[0..4] == &[0x8e, 0xd8, 0xa7, 0x0c] {
            return Some((
                "swapTokensForExactTokens".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
            ));
        }

        // Check for Uniswap V3 exactInputSingle function signature
        if input.len() >= 4 && &input[0..4] == &[0x41, 0x4b, 0xf3, 0x89] {
            return Some((
                "exactInputSingle".to_string(),
                "unknown".to_string(),
                "unknown".to_string(),
            ));
        }

        None
    }
}

#[async_trait]
impl Collector for MempoolCollector {
    fn name(&self) -> &str {
        "MempoolCollector"
    }

    fn network(&self) -> Network {
        self.network
    }

    async fn start(&mut self) -> ArbitrageResult<()> {
        if self.running {
            warn!("Mempool collector is already running");
            return Ok(());
        }

        info!("Starting mempool collector for network {}", self.network.name());
        self.running = true;
        Ok(())
    }

    async fn stop(&mut self) -> ArbitrageResult<()> {
        if !self.running {
            warn!("Mempool collector is not running");
            return Ok(());
        }

        info!("Stopping mempool collector for network {}", self.network.name());
        self.running = false;
        Ok(())
    }

    async fn collect(&self) -> ArbitrageResult<Vec<OpportunityData>> {
        if !self.running {
            return Ok(Vec::new());
        }

        // Monitor pending transactions
        let opportunities = self.monitor_pending_transactions().await?;

        // Sleep for the interval
        time::sleep(self.interval).await;

        Ok(opportunities)
    }
}
