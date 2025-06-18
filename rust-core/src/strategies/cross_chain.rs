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

/// Cross-chain arbitrage strategy for finding arbitrage opportunities across different chains
pub struct CrossChainArbitrageStrategy {
    app_state: Arc<AppState>,
    source_network: Network,
    target_network: Network,
    min_profit_threshold: f64,
}

impl CrossChainArbitrageStrategy {
    /// Create a new cross-chain arbitrage strategy
    pub fn new(
        app_state: Arc<AppState>,
        source_network: Network,
        target_network: Network,
        min_profit_threshold: f64,
    ) -> Self {
        Self {
            app_state,
            source_network,
            target_network,
            min_profit_threshold,
        }
    }

    /// Get all pools for a token on a network
    async fn get_token_pools(
        &self,
        network: Network,
        token: Address,
    ) -> ArbitrageResult<HashMap<String, Vec<Pool>>> {
        let pools = self.app_state.pools.read().await;
        let mut dex_pools = HashMap::new();

        for pool in pools.values() {
            if pool.network != network {
                continue;
            }

            // Check if the pool contains the token
            let has_token = pool.tokens.iter().any(|t| t.address == token);

            if has_token {
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

    /// Get the bridge fee for a token transfer between networks
    fn get_bridge_fee(
        &self,
        source_network: Network,
        target_network: Network,
        token: Address,
        amount: U256,
    ) -> ArbitrageResult<U256> {
        // In a real implementation, you would get the actual bridge fee from a bridge provider
        // For now, we'll use a placeholder
        let fee_percentage = match (source_network, target_network) {
            (Network::Mainnet, Network::Arbitrum) => 0.002, // 0.2%
            (Network::Mainnet, Network::Optimism) => 0.002, // 0.2%
            (Network::Mainnet, Network::Polygon) => 0.003, // 0.3%
            (Network::Mainnet, Network::Base) => 0.002, // 0.2%
            (Network::Arbitrum, Network::Mainnet) => 0.002, // 0.2%
            (Network::Optimism, Network::Mainnet) => 0.002, // 0.2%
            (Network::Polygon, Network::Mainnet) => 0.003, // 0.3%
            (Network::Base, Network::Mainnet) => 0.002, // 0.2%
            _ => 0.005, // 0.5% for other combinations
        };

        let fee = (amount.as_u128() as f64 * fee_percentage) as u128;
        Ok(U256::from(fee))
    }

    /// Find cross-chain arbitrage opportunities for a token
    async fn find_cross_chain_opportunities(
        &self,
        token: Address,
        input_amount: U256,
    ) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        debug!(
            "Finding cross-chain arbitrage opportunities for token: {:?}",
            token
        );

        // Get token details on source network
        let source_token_details = match self
            .app_state
            .get_token_by_address(self.source_network, token)
            .await
        {
            Some(token) => token,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Token details not found for token: {:?} on network: {}",
                    token,
                    self.source_network.name()
                )));
            }
        };

        // Get token price on source network
        let source_token_price = match self.app_state.token_prices.read().await.get(&token) {
            Some(price) => price.price_usd,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Token price not found for token: {:?} on network: {}",
                    token,
                    self.source_network.name()
                )));
            }
        };

        // Get the equivalent token on the target network
        let target_token = match self
            .app_state
            .get_equivalent_token(self.source_network, self.target_network, token)
            .await
        {
            Some(token) => token,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Equivalent token not found for token: {:?} on network: {}",
                    token,
                    self.target_network.name()
                )));
            }
        };

        // Get token details on target network
        let target_token_details = match self
            .app_state
            .get_token_by_address(self.target_network, target_token)
            .await
        {
            Some(token) => token,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Token details not found for token: {:?} on network: {}",
                    target_token,
                    self.target_network.name()
                )));
            }
        };

        // Get token price on target network
        let target_token_price = match self.app_state.token_prices.read().await.get(&target_token) {
            Some(price) => price.price_usd,
            None => {
                return Err(ArbitrageError::StrategyError(format!(
                    "Token price not found for token: {:?} on network: {}",
                    target_token,
                    self.target_network.name()
                )));
            }
        };

        // Calculate the price difference
        let price_difference = (target_token_price - source_token_price).abs();
        let price_ratio = if source_token_price > 0.0 {
            target_token_price / source_token_price
        } else {
            0.0
        };

        // Check if the price difference is significant
        if price_difference / source_token_price < 0.01 {
            // Less than 1% difference, not worth arbitraging
            return Ok(Vec::new());
        }

        // Calculate the expected output amount on the target network
        let expected_output = (input_amount.as_u128() as f64 * price_ratio) as u128;
        let expected_output = U256::from(expected_output);

        // Calculate the bridge fee
        let bridge_fee = self.get_bridge_fee(
            self.source_network,
            self.target_network,
            token,
            input_amount,
        )?;

        // Calculate the profit
        let profit_amount = if expected_output > input_amount + bridge_fee {
            expected_output - input_amount - bridge_fee
        } else {
            U256::zero()
        };

        // Calculate the profit in USD
        let profit_usd = (profit_amount.as_u128() as f64
            / 10f64.powi(source_token_details.decimals as i32))
            * source_token_price;

        // Check if the profit meets the threshold
        if profit_usd < self.min_profit_threshold {
            return Ok(Vec::new());
        }

        // Create the arbitrage opportunity
        let route = vec![
            SwapStep {
                dex: "bridge".to_string(),
                pool_address: Address::zero(),
                token_in: source_token_details.clone(),
                token_out: target_token_details.clone(),
                amount_in: input_amount,
                expected_amount_out: expected_output,
            },
        ];

        let opportunity = ArbitrageOpportunity {
            id: Uuid::new_v4().to_string(),
            network: self.source_network,
            route,
            input_token: source_token_details.clone(),
            input_amount,
            expected_output,
            expected_profit: profit_amount,
            expected_profit_usd: profit_usd,
            gas_cost_usd: 0.0, // Will be estimated by the simulator
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            status: "pending".to_string(),
        };

        Ok(vec![opportunity])
    }

    /// Find cross-chain arbitrage opportunities for all tokens
    async fn find_all_cross_chain_opportunities(
        &self,
        input_amount: U256,
    ) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        debug!(
            "Finding cross-chain arbitrage opportunities from {} to {}",
            self.source_network.name(),
            self.target_network.name()
        );

        // Get all tokens on the source network
        let mut tokens = HashSet::new();
        let pools = self.app_state.pools.read().await;
        for pool in pools.values() {
            if pool.network == self.source_network {
                for token in &pool.tokens {
                    tokens.insert(token.address);
                }
            }
        }

        // Find opportunities for each token
        let mut all_opportunities = Vec::new();
        for token in tokens {
            let opportunities = self.find_cross_chain_opportunities(token, input_amount).await?;
            all_opportunities.extend(opportunities);
        }

        debug!(
            "Found {} cross-chain arbitrage opportunities",
            all_opportunities.len()
        );
        Ok(all_opportunities)
    }
}

#[async_trait]
impl super::Strategy for CrossChainArbitrageStrategy {
    async fn find_opportunities(&self) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        debug!(
            "Finding cross-chain arbitrage opportunities from {} to {}",
            self.source_network.name(),
            self.target_network.name()
        );

        // Use a standard input amount (e.g., 1 ETH)
        let input_amount = U256::from(10).pow(U256::from(18));
        self.find_all_cross_chain_opportunities(input_amount).await
    }

    fn name(&self) -> &str {
        "CrossChainArbitrageStrategy"
    }
}
