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

/// Triangular arbitrage strategy for finding arbitrage opportunities within a single DEX
pub struct TriangularArbitrageStrategy {
    app_state: Arc<AppState>,
    network: Network,
    min_profit_threshold: f64,
}

impl TriangularArbitrageStrategy {
    /// Create a new triangular arbitrage strategy
    pub fn new(app_state: Arc<AppState>, network: Network, min_profit_threshold: f64) -> Self {
        Self {
            app_state,
            network,
            min_profit_threshold,
        }
    }

    /// Get all pools for a DEX
    async fn get_dex_pools(&self, dex: &str) -> ArbitrageResult<Vec<Pool>> {
        let pools = self.app_state.pools.read().await;
        let mut dex_pools = Vec::new();

        for pool in pools.values() {
            if pool.network == self.network && pool.dex.to_lowercase() == dex.to_lowercase() {
                dex_pools.push(pool.clone());
            }
        }

        Ok(dex_pools)
    }

    /// Build a graph of token pairs for a DEX
    fn build_token_graph(
        &self,
        pools: &[Pool],
    ) -> HashMap<Address, HashMap<Address, (Pool, bool)>> {
        let mut graph = HashMap::new();

        for pool in pools {
            if pool.tokens.len() != 2 {
                // Skip pools with more than 2 tokens (e.g., Curve pools)
                continue;
            }

            let token0 = pool.tokens[0].address;
            let token1 = pool.tokens[1].address;

            // Add token0 -> token1 edge
            graph
                .entry(token0)
                .or_insert_with(HashMap::new)
                .insert(token1, (pool.clone(), false));

            // Add token1 -> token0 edge
            graph
                .entry(token1)
                .or_insert_with(HashMap::new)
                .insert(token0, (pool.clone(), true));
        }

        graph
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
            _ => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Unsupported DEX type: {}",
                    pool.dex
                )));
            }
        };

        Ok(output_amount)
    }

    /// Find triangular arbitrage opportunities for a DEX
    async fn find_triangular_opportunities(
        &self,
        dex: &str,
        input_amount: U256,
    ) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        debug!("Finding triangular arbitrage opportunities for DEX: {}", dex);

        // Get all pools for the DEX
        let pools = self.get_dex_pools(dex).await?;
        if pools.is_empty() {
            debug!("No pools found for DEX: {}", dex);
            return Ok(Vec::new());
        }

        // Build a graph of token pairs
        let graph = self.build_token_graph(&pools);

        // Get all tokens in the DEX
        let mut tokens = HashSet::new();
        for pool in &pools {
            for token in &pool.tokens {
                tokens.insert(token.address);
            }
        }

        // Find triangular arbitrage opportunities
        let mut opportunities = Vec::new();

        // For each token, find triangular paths
        for &start_token in &tokens {
            // Get the token details
            let token_details = match self
                .app_state
                .get_token_by_address(self.network, start_token)
                .await
            {
                Some(token) => token,
                None => continue, // Skip if token details not found
            };

            // Skip tokens with no price
            let token_price = match self.app_state.token_prices.read().await.get(&start_token) {
                Some(price) => price.price_usd,
                None => continue, // Skip if token price not found
            };

            // Calculate the input amount in the token's decimals
            let decimals = token_details.decimals;
            let input_amount_adjusted =
                input_amount * U256::from(10).pow(U256::from(decimals as u64));

            // Find triangular paths
            self.find_triangular_paths(
                &graph,
                start_token,
                start_token,
                input_amount_adjusted,
                Vec::new(),
                &mut opportunities,
                &token_details,
                token_price,
                dex,
                2, // Maximum path length (3 tokens = 2 swaps)
            )?;
        }

        debug!(
            "Found {} triangular arbitrage opportunities for DEX: {}",
            opportunities.len(),
            dex
        );
        Ok(opportunities)
    }

    /// Find triangular paths in the token graph
    #[allow(clippy::too_many_arguments)]
    fn find_triangular_paths(
        &self,
        graph: &HashMap<Address, HashMap<Address, (Pool, bool)>>,
        start_token: Address,
        current_token: Address,
        current_amount: U256,
        path: Vec<(Address, Address, Pool, bool)>,
        opportunities: &mut Vec<ArbitrageOpportunity>,
        start_token_details: &Token,
        start_token_price: f64,
        dex: &str,
        max_depth: usize,
    ) -> ArbitrageResult<()> {
        // If we've reached the maximum depth, check if we can complete the cycle
        if path.len() >= max_depth {
            // Check if we can go back to the start token
            if let Some(neighbors) = graph.get(&current_token) {
                if let Some((pool, reversed)) = neighbors.get(&start_token) {
                    // Calculate the final output amount
                    let output_amount = self.calculate_output_amount(
                        pool,
                        current_token,
                        start_token,
                        current_amount,
                    )?;

                    // Check if the arbitrage is profitable
                    if output_amount > path[0].2.min_trade_amount {
                        let profit_amount = if output_amount > path[0].2.min_trade_amount {
                            output_amount - path[0].2.min_trade_amount
                        } else {
                            U256::zero()
                        };

                        // Calculate the profit in USD
                        let profit_usd = (profit_amount.as_u128() as f64
                            / 10f64.powi(start_token_details.decimals as i32))
                            * start_token_price;

                        // Check if the profit meets the threshold
                        if profit_usd >= self.min_profit_threshold {
                            // Create the arbitrage opportunity
                            let mut route = Vec::new();

                            // Add the path steps
                            for (token_in, token_out, pool, reversed) in &path {
                                let token_in_details = self
                                    .app_state
                                    .get_token_by_address(self.network, *token_in)
                                    .await
                                    .unwrap_or_else(|| Token {
                                        address: *token_in,
                                        symbol: format!("Unknown-{:?}", token_in),
                                        name: format!("Unknown Token {:?}", token_in),
                                        decimals: 18,
                                        network: self.network,
                                    });

                                let token_out_details = self
                                    .app_state
                                    .get_token_by_address(self.network, *token_out)
                                    .await
                                    .unwrap_or_else(|| Token {
                                        address: *token_out,
                                        symbol: format!("Unknown-{:?}", token_out),
                                        name: format!("Unknown Token {:?}", token_out),
                                        decimals: 18,
                                        network: self.network,
                                    });

                                route.push(SwapStep {
                                    dex: dex.to_string(),
                                    pool_address: pool.address,
                                    token_in: token_in_details,
                                    token_out: token_out_details,
                                    amount_in: U256::zero(), // Will be filled in later
                                    expected_amount_out: U256::zero(), // Will be filled in later
                                });
                            }

                            // Add the final step back to the start token
                            let final_token_in_details = self
                                .app_state
                                .get_token_by_address(self.network, current_token)
                                .await
                                .unwrap_or_else(|| Token {
                                    address: current_token,
                                    symbol: format!("Unknown-{:?}", current_token),
                                    name: format!("Unknown Token {:?}", current_token),
                                    decimals: 18,
                                    network: self.network,
                                });

                            route.push(SwapStep {
                                dex: dex.to_string(),
                                pool_address: pool.address,
                                token_in: final_token_in_details,
                                token_out: start_token_details.clone(),
                                amount_in: current_amount,
                                expected_amount_out: output_amount,
                            });

                            // Fill in the amounts for each step
                            let mut current_amount = path[0].2.min_trade_amount;
                            for i in 0..route.len() - 1 {
                                route[i].amount_in = current_amount;
                                current_amount = self.calculate_output_amount(
                                    &route[i].pool_address,
                                    route[i].token_in.address,
                                    route[i].token_out.address,
                                    current_amount,
                                )?;
                                route[i].expected_amount_out = current_amount;
                            }

                            // Create the opportunity
                            let opportunity = ArbitrageOpportunity {
                                id: Uuid::new_v4().to_string(),
                                network: self.network,
                                route,
                                input_token: start_token_details.clone(),
                                input_amount: path[0].2.min_trade_amount,
                                expected_output: output_amount,
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
            return Ok(());
        }

        // Explore neighbors
        if let Some(neighbors) = graph.get(&current_token) {
            for (&next_token, (pool, reversed)) in neighbors {
                // Skip if we've already visited this token in the path
                if path.iter().any(|(_, token_out, _, _)| *token_out == next_token) {
                    continue;
                }

                // Calculate the output amount
                let output_amount = self.calculate_output_amount(
                    pool,
                    current_token,
                    next_token,
                    current_amount,
                )?;

                // Skip if the output amount is too small
                if output_amount <= U256::zero() {
                    continue;
                }

                // Add the step to the path
                let mut new_path = path.clone();
                new_path.push((current_token, next_token, pool.clone(), *reversed));

                // Continue exploring
                self.find_triangular_paths(
                    graph,
                    start_token,
                    next_token,
                    output_amount,
                    new_path,
                    opportunities,
                    start_token_details,
                    start_token_price,
                    dex,
                    max_depth,
                )?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl super::Strategy for TriangularArbitrageStrategy {
    async fn find_opportunities(&self) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        debug!(
            "Finding triangular arbitrage opportunities for network: {}",
            self.network.name()
        );

        // Get all DEXes on the network
        let mut dexes = HashSet::new();
        let pools = self.app_state.pools.read().await;
        for pool in pools.values() {
            if pool.network == self.network {
                dexes.insert(pool.dex.clone());
            }
        }

        // Find opportunities for each DEX
        let mut all_opportunities = Vec::new();
        for dex in dexes {
            // Use a standard input amount (e.g., 1 ETH)
            let input_amount = U256::from(10).pow(U256::from(18));
            let opportunities = self.find_triangular_opportunities(&dex, input_amount).await?;
            all_opportunities.extend(opportunities);
        }

        debug!(
            "Found {} triangular arbitrage opportunities",
            all_opportunities.len()
        );
        Ok(all_opportunities)
    }

    fn name(&self) -> &str {
        "TriangularArbitrageStrategy"
    }
}
