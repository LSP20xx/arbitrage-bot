use crate::common::{AppState, ArbitrageError, ArbitrageOpportunity, ArbitrageResult, Network};
use async_trait::async_trait;
use ethers::abi::{Abi, Token as AbiToken};
use ethers::contract::Contract;
use ethers::prelude::*;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Local simulator for arbitrage simulation
pub struct LocalSimulator {
    app_state: Arc<AppState>,
}

impl LocalSimulator {
    /// Create a new local simulator
    pub fn new(app_state: Arc<AppState>) -> Self {
        Self { app_state }
    }

    /// Simulate a swap on Uniswap V2
    async fn simulate_uniswap_v2_swap(
        &self,
        router_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        provider: Arc<Provider<Ws>>,
    ) -> ArbitrageResult<U256> {
        debug!(
            "Simulating Uniswap V2 swap: {} -> {} with amount {}",
            token_in, token_out, amount_in
        );

        // Load the Uniswap V2 router ABI
        let router_abi: Abi = serde_json::from_str(include_str!("../abis/uniswap_v2_router.json"))?;
        let router = Contract::new(router_address, router_abi, provider);

        // Call getAmountsOut function
        let amounts: Vec<U256> = router
            .method::<_, Vec<U256>>(
                "getAmountsOut",
                (
                    amount_in,
                    vec![token_in, token_out], // Path
                ),
            )?
            .call()
            .await?;

        // Return the output amount
        if amounts.len() < 2 {
            return Err(ArbitrageError::SimulationError(
                "Invalid response from Uniswap V2 router".to_string(),
            ));
        }

        Ok(amounts[1])
    }

    /// Simulate a swap on Uniswap V3
    async fn simulate_uniswap_v3_swap(
        &self,
        router_address: Address,
        token_in: Address,
        token_out: Address,
        amount_in: U256,
        provider: Arc<Provider<Ws>>,
    ) -> ArbitrageResult<U256> {
        debug!(
            "Simulating Uniswap V3 swap: {} -> {} with amount {}",
            token_in, token_out, amount_in
        );

        // Load the Uniswap V3 quoter ABI
        let quoter_abi: Abi = serde_json::from_str(include_str!("../abis/uniswap_v3_quoter.json"))?;
        let quoter = Contract::new(router_address, quoter_abi, provider);

        // Call quoteExactInputSingle function
        let amount_out: U256 = quoter
            .method::<_, U256>(
                "quoteExactInputSingle",
                (
                    token_in,
                    token_out,
                    3000u32, // Fee (0.3%)
                    amount_in,
                    0u128, // SqrtPriceLimitX96
                ),
            )?
            .call()
            .await?;

        Ok(amount_out)
    }

    /// Simulate a swap on Curve
    async fn simulate_curve_swap(
        &self,
        pool_address: Address,
        i: i128,
        j: i128,
        amount_in: U256,
        provider: Arc<Provider<Ws>>,
    ) -> ArbitrageResult<U256> {
        debug!(
            "Simulating Curve swap: {} -> {} with amount {}",
            i, j, amount_in
        );

        // Load the Curve pool ABI
        let pool_abi: Abi = serde_json::from_str(include_str!("../abis/curve_pool.json"))?;
        let pool = Contract::new(pool_address, pool_abi, provider);

        // Call get_dy function
        let amount_out: U256 = pool
            .method::<_, U256>("get_dy", (i, j, amount_in))?
            .call()
            .await?;

        Ok(amount_out)
    }

    /// Simulate an arbitrage opportunity
    async fn simulate_opportunity(
        &self,
        opportunity: &ArbitrageOpportunity,
    ) -> ArbitrageResult<(bool, U256, Option<String>)> {
        debug!("Simulating arbitrage opportunity: {}", opportunity.id);

        // Get the provider for the network
        let provider = match self.app_state.providers.get(&opportunity.network) {
            Some(provider) => provider.clone(),
            None => {
                return Err(ArbitrageError::SimulationError(format!(
                    "No provider available for network {}",
                    opportunity.network.name()
                )));
            }
        };

        // Simulate each step in the route
        let mut current_amount = opportunity.input_amount;
        let mut error = None;

        for (i, step) in opportunity.route.iter().enumerate() {
            debug!(
                "Simulating step {}: {} -> {} with amount {}",
                i, step.token_in.symbol, step.token_out.symbol, current_amount
            );

            // Determine the DEX type and simulate the swap
            let dex_type = step.dex.to_lowercase();
            let amount_out = match dex_type.as_str() {
                "uniswap_v2" | "sushiswap" => {
                    self.simulate_uniswap_v2_swap(
                        step.pool_address,
                        step.token_in.address,
                        step.token_out.address,
                        current_amount,
                        provider.clone(),
                    )
                    .await
                }
                "uniswap_v3" => {
                    self.simulate_uniswap_v3_swap(
                        step.pool_address,
                        step.token_in.address,
                        step.token_out.address,
                        current_amount,
                        provider.clone(),
                    )
                    .await
                }
                "curve" => {
                    // For Curve, we need to determine the token indices
                    // In a real implementation, you would get these from the pool configuration
                    let i = 0i128; // Placeholder
                    let j = 1i128; // Placeholder
                    self.simulate_curve_swap(step.pool_address, i, j, current_amount, provider.clone())
                        .await
                }
                _ => {
                    return Err(ArbitrageError::SimulationError(format!(
                        "Unsupported DEX type: {}",
                        dex_type
                    )));
                }
            };

            // Check if the swap was successful
            match amount_out {
                Ok(amount) => {
                    debug!(
                        "Step {} simulation result: {} -> {}",
                        i, current_amount, amount
                    );
                    current_amount = amount;
                }
                Err(e) => {
                    error = Some(format!("Step {} failed: {}", i, e));
                    return Ok((false, U256::zero(), error));
                }
            }
        }

        // Check if the arbitrage is profitable
        let is_profitable = current_amount > opportunity.input_amount;
        debug!(
            "Arbitrage simulation result: {} -> {} (profitable: {})",
            opportunity.input_amount, current_amount, is_profitable
        );

        Ok((is_profitable, current_amount, error))
    }
}

#[async_trait]
impl super::Simulator for LocalSimulator {
    async fn simulate(
        &self,
        opportunity: ArbitrageOpportunity,
    ) -> ArbitrageResult<super::SimulationResult> {
        debug!(
            "Simulating arbitrage opportunity {} with local simulator",
            opportunity.id
        );

        // Simulate the opportunity
        let (success, actual_output, error) = self.simulate_opportunity(&opportunity).await?;

        // Estimate gas cost
        // In a real implementation, you would estimate the gas cost based on the transaction
        let gas_used = U256::from(500000); // Placeholder

        // Create the simulation result
        let result = super::SimulationResult {
            opportunity: opportunity.clone(),
            success,
            actual_output: Some(actual_output),
            gas_used: Some(gas_used),
            error: error.map(|e| e.to_string()),
            trace: None,
        };

        Ok(result)
    }

    fn name(&self) -> &str {
        "LocalSimulator"
    }
}
