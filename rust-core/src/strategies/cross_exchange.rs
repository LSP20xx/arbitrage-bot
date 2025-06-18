use crate::common::{
    AppState, ArbitrageError, ArbitrageOpportunity, ArbitrageResult, Network, Pool, SwapStep, Token,
};
use async_trait::async_trait;
use ethers::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Cross-exchange arbitrage strategy for finding arbitrage opportunities across different DEXes
pub struct CrossExchangeArbitrageStrategy {
    app_state: Arc<AppState>,
    network: Network,
    min_profit_threshold: f64,
}

impl CrossExchangeArbitrageStrategy {
    /// Create a new cross-exchange arbitrage strategy
    pub fn new(app_state: Arc<AppState>, network: Network, min_profit_threshold: f64) -> Self {
        Self {
            app_state,
            network,
            min_profit_threshold,
        }
    }

    /// Get all pools for a token pair across different DEXes
    async fn get_token_pair_pools(
        &self,
        token_a: Address,
        token_b: Address,
    ) -> ArbitrageResult<HashMap<String, Vec<Pool>>> {
        let pools = self.app_state.pools.read().await;
        let mut dex_pools = HashMap::new();

        for pool in pools.values() {
            if pool.network != self.network {
                continue;
            }

            // Check if the pool contains both tokens
            let has_token_a = pool.tokens.iter().any(|t| t.address == token_a);
            let has_token_b = pool.tokens.iter().any(|t| t.address == token_b);

            if has_token_a && has_token_b {
                dex_pools
                    .entry(pool.dex.clone())
                    .or_insert_with(Vec::new)
                    .push(pool.clone());
            }
        }

        Ok(dex_pools)
    }

    /// Calculate the output amount for a swap
    fn calculate_output_amount(
        &self,
        pool: &Pool,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> ArbitrageResult<U256> {
        // Find the indices of the tokens in the pool
        let token_in_idx = pool
            .tokens
            .iter()
            .position(|t| t.address == token_in)
            .ok_or_else(|| {
                ArbitrageError::StrategyError(format!(
                    "Token {} not found in pool {}",
                    token_in, pool.address
                ))
            })?;
        let token_out_idx = pool
            .tokens
            .iter()
            .position(|t| t.address == token_out)
            .ok_or_else(|| {
                ArbitrageError::StrategyError(format!(
                    "Token {} not found in pool {}",
                    token_out, pool.address
                ))
            })?;

        // Calculate the output amount based on the pool type
        let output_amount = match pool.dex.to_lowercase().as_str() {
            "uniswap_v2" | "sushiswap" => {
                // Uniswap V2 formula: amount_out = amount_in * reserve_out / (reserve_in + amount_in)
                let reserve_in = pool.reserves[token_in_idx];
                let reserve_out = pool.reserves[token_out_idx];

                // Calculate the output amount
                let amount_in_with_fee = amount_in * U256::from(997); // 0.3% fee
                let numerator = amount_in_with_fee * reserve_out;
                let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;
                numerator / denominator
            }
            "uniswap_v3" => {
                // For Uniswap V3, we would need to use the sqrt price and liquidity
                // This is a simplified calculation
                let reserve_in = pool.reserves[token_in_idx];
                let reserve_out = pool.reserves[token_out_idx];

                // Calculate the output amount (simplified)
                let amount_in_with_fee = amount_in * U256::from(997); // 0.3% fee
                let numerator = amount_in_with_fee * reserve_out;
                let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;
                numerator / denominator
            }
            "curve" => {
                // For Curve, we would need to use the specific curve formula
                // This is a simplified calculation
                let reserve_in = pool.reserves[token_in_idx];
                let reserve_out = pool.reserves[token_out_idx];

                // Calculate the output amount (simplified)
                let amount_in_with_fee = amount_in * U256::from(999); // 0.1% fee
                let numerator = amount_in_with_fee * reserve_out;
                let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;
                numerator / denominator
            }
            "balancer" => {
                // For Balancer, we would need to use the specific balancer formula
                // This is a simplified calculation
                let reserve_in = pool.reserves[token_in_idx];
                let reserve_out = pool.reserves[token_out_idx];

                // Calculate the output amount (simplified)
                let amount_in_with_fee = amount_in * U256::from(998); // 0.2% fee
                let numerator = amount_in_with_fee * reserve_out;
                let denominator = reserve_in * U256::from(1000) + amount_in_with_fee;
                numerator / denominator
            }
            _ => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Unsupported DEX type: {}",
                    pool.dex
                )));
            }
        };

        Ok(output_amount)
    }

    /// Find the best pool for a token pair on a DEX
    fn find_best_pool(
        &self,
        pools: &[Pool],
        token_in: Address,
        token_out: Address,
        amount_in: U256,
    ) -> ArbitrageResult<Option<(Pool, U256)>> {
        let mut best_pool = None;
        let mut best_output_amount = U256::zero();

        for pool in pools {
            // Check if the pool contains both tokens
            let has_token_in = pool.tokens.iter().any(|t| t.address == token_in);
            let has_token_out = pool.tokens.iter().any(|t| t.address == token_out);

            if !has_token_in || !has_token_out {
                continue;
            }

            // Calculate the output amount
            let output_amount = self.calculate_output_amount(pool, token_in, token_out, amount_in)?;

            // Update the best pool if this one is better
            if output_amount > best_output_amount {
                best_pool = Some(pool.clone());
                best_output_amount = output_amount;
            }
        }

        if let Some(pool) = best_pool {
            Ok(Some((pool, best_output_amount)))
        } else {
            Ok(None)
        }
    }

    /// Find cross-exchange arbitrage opportunities for a token pair
    async fn find_cross_exchange_opportunities(
        &self,
        token_a: Address,
        token_b: Address,
        input_amount: U256,
    ) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        debug!(
            "Finding cross-exchange arbitrage opportunities for token pair: {:?} - {:?}",
            token_a, token_b
        );

        // Get all pools for the token pair across different DEXes
        let dex_pools = self.get_token_pair_pools(token_a, token_b).await?;
        if dex_pools.is_empty() {
            debug!("No pools found for token pair: {:?} - {:?}", token_a, token_b);
            return Ok(Vec::new());
        }

        // Get token details
        let token_a_details = match self
            .app_state
            .get_token_by_address(self.network, token_a)
            .await
        {
            Some(token) => token,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Token details not found for token: {:?}",
                    token_a
                )));
            }
        };

        let token_b_details = match self
            .app_state
            .get_token_by_address(self.network, token_b)
            .await
        {
            Some(token) => token,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Token details not found for token: {:?}",
                    token_b
                )));
            }
        };

        // Get token prices
        let token_a_price = match self.app_state.token_prices.read().await.get(&token_a) {
            Some(price) => price.price_usd,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Token price not found for token: {:?}",
                    token_a
                )));
            }
        };

        // Find arbitrage opportunities
        let mut opportunities = Vec::new();

        // For each DEX pair, check if there's an arbitrage opportunity
        let dexes: Vec<String> = dex_pools.keys().cloned().collect();
        for i in 0..dexes.len() {
            for j in 0..dexes.len() {
                if i == j {
                    continue;
                }

                let dex_a = &dexes[i];
                let dex_b = &dexes[j];

                // Find the best pool for buying token_b with token_a on dex_a
                let pools_a = &dex_pools[dex_a];
                let buy_result = self.find_best_pool(pools_a, token_a, token_b, input_amount)?;

                if let Some((buy_pool, output_amount)) = buy_result {
                    // Find the best pool for selling token_b back to token_a on dex_b
                    let pools_b = &dex_pools[dex_b];
                    let sell_result = self.find_best_pool(pools_b, token_b, token_a, output_amount)?;

                    if let Some((sell_pool, final_amount)) = sell_result {
                        // Check if the arbitrage is profitable
                        if final_amount > input_amount {
                            let profit_amount = final_amount - input_amount;

                            // Calculate the profit in USD
                            let profit_usd = (profit_amount.as_u128() as f64
                                / 10f64.powi(token_a_details.decimals as i32))
                                * token_a_price;

                            // Check if the profit meets the threshold
                            if profit_usd >= self.min_profit_threshold {
                                // Create the arbitrage opportunity
                                let route = vec![
                                    SwapStep {
                                        dex: dex_a.clone(),
                                        pool_address: buy_pool.address,
                                        token_in: token_a_details.clone(),
                                        token_out: token_b_details.clone(),
                                        amount_in: input_amount,
                                        expected_amount_out: output_amount,
                                    },
                                    SwapStep {
                                        dex: dex_b.clone(),
                                        pool_address: sell_pool.address,
                                        token_in: token_b_details.clone(),
                                        token_out: token_a_details.clone(),
                                        amount_in: output_amount,
                                        expected_amount_out: final_amount,
                                    },
                                ];

                                let opportunity = ArbitrageOpportunity {
                                    id: Uuid::new_v4().to_string(),
                                    network: self.network,
                                    route,
                                    input_token: token_a_details.clone(),
                                    input_amount,
                                    expected_output: final_amount,
                                    expected_profit: profit_amount,
                                    expected_profit_usd: profit_usd,
                                    gas_cost_usd: 0.0, // Will be estimated by the simulator
                                    timestamp: SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs(),
                                    status: "pending".to_string(),
                                };

                                opportunities.push(opportunity);
                            }
                        }
                    }
                }
            }
        }

        debug!(
            "Found {} cross-exchange arbitrage opportunities for token pair: {:?} - {:?}",
            opportunities.len(),
            token_a,
            token_b
        );
        Ok(opportunities)
    }
}

#[async_trait]
impl super::Strategy for CrossExchangeArbitrageStrategy {
    async fn find_opportunities(&self) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        debug!(
            "Finding cross-exchange arbitrage opportunities for network: {}",
            self.network.name()
        );

        // Get all token pairs on the network
        let mut token_pairs = HashSet::new();
        let pools = self.app_state.pools.read().await;
        for pool in pools.values() {
            if pool.network == self.network && pool.tokens.len() == 2 {
                let token_a = pool.tokens[0].address;
                let token_b = pool.tokens[1].address;
                token_pairs.insert((token_a, token_b));
            }
        }

        // Find opportunities for each token pair
        let mut all_opportunities = Vec::new();
        for (token_a, token_b) in token_pairs {
            // Use a standard input amount (e.g., 1 ETH)
            let input_amount = U256::from(10).pow(U256::from(18));
            let opportunities = self
                .find_cross_exchange_opportunities(token_a, token_b, input_amount)
                .await?;
            all_opportunities.extend(opportunities);
        }

        debug!(
            "Found {} cross-exchange arbitrage opportunities",
            all_opportunities.len()
        );
        Ok(all_opportunities)
    }

    fn name(&self) -> &str {
        "CrossExchangeArbitrageStrategy"
    }
}
