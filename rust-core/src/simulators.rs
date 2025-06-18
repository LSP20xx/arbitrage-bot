use crate::common::{
    ArbitrageError, ArbitrageOpportunity, DexInfo, DexType, Network, SimulationResult, TokenInfo,
};
use async_trait::async_trait;
use ethers::abi::{encode_packed, Token};
use ethers::prelude::*;
use ethers::types::{Address, Bytes, H256, U256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, error, info, warn};

/// Simulator trait for simulating arbitrage opportunities
#[async_trait]
pub trait Simulator: Send + Sync {
    /// Simulate an arbitrage opportunity
    async fn simulate(
        &self,
        opportunity: &ArbitrageOpportunity,
    ) -> Result<SimulationResult, ArbitrageError>;

    /// Get the simulator name
    fn name(&self) -> &str;

    /// Get the simulator type
    fn simulator_type(&self) -> SimulatorType;
}

/// Simulator types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimulatorType {
    Tenderly,
    Local,
    ForkNode,
    Custom,
}

/// Tenderly simulator configuration
#[derive(Debug, Clone)]
pub struct TenderlySimulatorConfig {
    pub api_key: String,
    pub account_id: String,
    pub project_slug: String,
    pub base_url: String,
    pub networks: HashMap<Network, String>, // Network -> Tenderly network name
    pub dexes: HashMap<Address, DexInfo>,
    pub tokens: HashMap<Address, TokenInfo>,
    pub gas_price: U256,
    pub timeout_ms: u64,
}

/// Tenderly simulator for simulating arbitrage opportunities using Tenderly API
pub struct TenderlySimulator {
    config: TenderlySimulatorConfig,
    client: reqwest::Client,
    name: String,
}

impl TenderlySimulator {
    /// Create a new Tenderly simulator
    pub fn new(config: TenderlySimulatorConfig) -> Self {
        let name = "tenderly_simulator".to_string();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()
            .unwrap_or_default();

        Self {
            config,
            client,
            name,
        }
    }

    /// Get the Tenderly network name for a network
    fn get_tenderly_network(&self, network: Network) -> Result<&str, ArbitrageError> {
        self.config
            .networks
            .get(&network)
            .map(|s| s.as_str())
            .ok_or_else(|| {
                ArbitrageError::ConfigurationError(format!(
                    "No Tenderly network mapping found for {}",
                    network
                ))
            })
    }

    /// Create a simulation request for Tenderly API
    fn create_simulation_request(
        &self,
        opportunity: &ArbitrageOpportunity,
    ) -> Result<serde_json::Value, ArbitrageError> {
        // Get the Tenderly network name
        let network_name = self.get_tenderly_network(opportunity.network)?;

        // Get the first token in the path
        let token_in = opportunity
            .token_path
            .first()
            .ok_or_else(|| ArbitrageError::SimulationError("Empty token path".to_string()))?;

        // Get the last token in the path
        let token_out = opportunity
            .token_path
            .last()
            .ok_or_else(|| ArbitrageError::SimulationError("Empty token path".to_string()))?;

        // Get the first DEX in the path
        let dex = opportunity
            .dex_path
            .first()
            .ok_or_else(|| ArbitrageError::SimulationError("Empty DEX path".to_string()))?;

        // Get DEX info
        let dex_info = self.config.dexes.get(dex).ok_or_else(|| {
            ArbitrageError::ConfigurationError(format!("No DEX info found for {}", dex))
        })?;

        // Get token info
        let token_in_info = self.config.tokens.get(token_in).ok_or_else(|| {
            ArbitrageError::ConfigurationError(format!("No token info found for {}", token_in))
        })?;

        // Create the transaction input data
        // This is a simplified example - in a real implementation, you would create the correct input data for the specific DEX and operation
        let input_data = match dex_info.dex_type {
            DexType::UniswapV2 => {
                // Example: swapExactTokensForTokens(amountIn, amountOutMin, path, to, deadline)
                let function_selector = "0x38ed1739"; // swapExactTokensForTokens
                let amount_in = opportunity.expected_profit; // Use expected profit as amount in for simulation
                let amount_out_min = U256::zero(); // No minimum output for simulation
                let path = opportunity.token_path.clone();
                let to = Address::zero(); // Dummy address for simulation
                let deadline = U256::from(u64::MAX); // Max deadline for simulation

                // Encode the function call
                let mut data = hex::decode(function_selector.trim_start_matches("0x"))
                    .map_err(|e| {
                        ArbitrageError::SimulationError(format!(
                            "Failed to decode function selector: {}",
                            e
                        ))
                    })?;

                // Encode the parameters
                let params = encode_packed(&[
                    Token::Uint(amount_in),
                    Token::Uint(amount_out_min),
                    Token::Array(
                        path.iter()
                            .map(|addr| Token::Address(*addr))
                            .collect::<Vec<_>>(),
                    ),
                    Token::Address(to),
                    Token::Uint(deadline),
                ])
                .map_err(|e| {
                    ArbitrageError::SimulationError(format!("Failed to encode parameters: {}", e))
                })?;

                data.extend_from_slice(&params);
                Bytes::from(data)
            }
            DexType::UniswapV3 => {
                // Example: exactInputSingle(params)
                let function_selector = "0x414bf389"; // exactInputSingle
                let amount_in = opportunity.expected_profit; // Use expected profit as amount in for simulation
                let amount_out_min = U256::zero(); // No minimum output for simulation
                let token_in = *token_in;
                let token_out = *token_out;
                let fee = U256::from(3000); // Default fee tier (0.3%)
                let to = Address::zero(); // Dummy address for simulation
                let deadline = U256::from(u64::MAX); // Max deadline for simulation
                let sqrt_price_limit_x96 = U256::zero(); // No price limit for simulation

                // Encode the function call
                let mut data = hex::decode(function_selector.trim_start_matches("0x"))
                    .map_err(|e| {
                        ArbitrageError::SimulationError(format!(
                            "Failed to decode function selector: {}",
                            e
                        ))
                    })?;

                // Encode the parameters
                let params = encode_packed(&[
                    Token::Address(token_in),
                    Token::Address(token_out),
                    Token::Uint(fee),
                    Token::Address(to),
                    Token::Uint(deadline),
                    Token::Uint(amount_in),
                    Token::Uint(amount_out_min),
                    Token::Uint(sqrt_price_limit_x96),
                ])
                .map_err(|e| {
                    ArbitrageError::SimulationError(format!("Failed to encode parameters: {}", e))
                })?;

                data.extend_from_slice(&params);
                Bytes::from(data)
            }
            DexType::Curve => {
                // Example: exchange(i, j, dx, min_dy)
                let function_selector = "0x3df02124"; // exchange
                let i = U256::zero(); // Index of input token (simplified)
                let j = U256::one(); // Index of output token (simplified)
                let dx = opportunity.expected_profit; // Use expected profit as amount in for simulation
                let min_dy = U256::zero(); // No minimum output for simulation

                // Encode the function call
                let mut data = hex::decode(function_selector.trim_start_matches("0x"))
                    .map_err(|e| {
                        ArbitrageError::SimulationError(format!(
                            "Failed to decode function selector: {}",
                            e
                        ))
                    })?;

                // Encode the parameters
                let params = encode_packed(&[
                    Token::Int(i.into()),
                    Token::Int(j.into()),
                    Token::Uint(dx),
                    Token::Uint(min_dy),
                ])
                .map_err(|e| {
                    ArbitrageError::SimulationError(format!("Failed to encode parameters: {}", e))
                })?;

                data.extend_from_slice(&params);
                Bytes::from(data)
            }
            _ => {
                return Err(ArbitrageError::SimulationError(format!(
                    "Unsupported DEX type: {:?}",
                    dex_info.dex_type
                )))
            }
        };

        // Create the simulation request
        let request = serde_json::json!({
            "network_id": network_name,
            "from": "0x0000000000000000000000000000000000000000", // Dummy address for simulation
            "to": dex_info.router_address,
            "input": input_data.to_string(),
            "gas": 8000000,
            "gas_price": format!("{:#x}", self.config.gas_price),
            "value": "0x0",
            "save": false,
            "save_if_fails": false,
            "state_objects": {
                token_in.to_string(): {
                    "balance": format!("{:#x}", opportunity.expected_profit),
                }
            }
        });

        Ok(request)
    }

    /// Send a simulation request to Tenderly API
    async fn send_simulation_request(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value, ArbitrageError> {
        // Create the API URL
        let url = format!(
            "{}/api/v1/account/{}/project/{}/simulate",
            self.config.base_url, self.config.account_id, self.config.project_slug
        );

        // Send the request
        let response = self
            .client
            .post(&url)
            .header("X-Access-Key", &self.config.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                ArbitrageError::NetworkError(format!("Failed to send simulation request: {}", e))
            })?;

        // Check if the request was successful
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ArbitrageError::SimulationError(format!(
                "Simulation request failed: {}",
                error_text
            )));
        }

        // Parse the response
        let response_json = response.json::<serde_json::Value>().await.map_err(|e| {
            ArbitrageError::SimulationError(format!("Failed to parse simulation response: {}", e))
        })?;

        Ok(response_json)
    }

    /// Parse the simulation response from Tenderly API
    fn parse_simulation_response(
        &self,
        response: serde_json::Value,
        opportunity: &ArbitrageOpportunity,
    ) -> Result<SimulationResult, ArbitrageError> {
        // Check if the simulation was successful
        let simulation = response
            .get("simulation")
            .ok_or_else(|| ArbitrageError::SimulationError("Missing simulation data".to_string()))?;

        let status = simulation
            .get("status")
            .and_then(|s| s.as_bool())
            .ok_or_else(|| {
                ArbitrageError::SimulationError("Missing simulation status".to_string())
            })?;

        // Get the gas used
        let gas_used = simulation
            .get("gas_used")
            .and_then(|g| g.as_str())
            .and_then(|g| g.trim_start_matches("0x").parse::<u64>().ok())
            .map(U256::from)
            .ok_or_else(|| ArbitrageError::SimulationError("Missing gas used".to_string()))?;

        // If the simulation failed, return an error
        if !status {
            let error = simulation
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error");

            return Ok(SimulationResult {
                opportunity_id: opportunity.id.clone(),
                success: false,
                expected_profit: None,
                expected_gas_used: Some(gas_used),
                error: Some(error.to_string()),
                simulation_time_ms: 0,
                timestamp: chrono::Utc::now(),
            });
        }

        // Get the last token in the path
        let token_out = opportunity
            .token_path
            .last()
            .ok_or_else(|| ArbitrageError::SimulationError("Empty token path".to_string()))?;

        // Get token info
        let token_out_info = self.config.tokens.get(token_out).ok_or_else(|| {
            ArbitrageError::ConfigurationError(format!("No token info found for {}", token_out))
        })?;

        // Get the call traces
        let call_traces = simulation
            .get("call_traces")
            .and_then(|c| c.as_array())
            .ok_or_else(|| {
                ArbitrageError::SimulationError("Missing call traces".to_string())
            })?;

        // Find the last call trace that transfers tokens to the user
        let mut output_amount = U256::zero();
        for trace in call_traces.iter().rev() {
            // Check if this is a token transfer
            let to = trace
                .get("to")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_lowercase();

            let input = trace.get("input").and_then(|i| i.as_str()).unwrap_or_default();

            // Check if this is a transfer call (simplified)
            if to == token_out.to_string().to_lowercase() && input.starts_with("0xa9059cbb") {
                // This is a transfer call, extract the amount
                if let Some(output) = trace.get("output").and_then(|o| o.as_str()) {
                    if output.len() >= 66 {
                        // Extract the amount from the output
                        if let Ok(amount) = U256::from_str_radix(&output[2..66], 16) {
                            output_amount = amount;
                            break;
                        }
                    }
                }
            }
        }

        // Calculate the profit
        let profit = if output_amount > opportunity.expected_profit {
            output_amount - opportunity.expected_profit
        } else {
            U256::zero()
        };

        // Create the simulation result
        let result = SimulationResult {
            opportunity_id: opportunity.id.clone(),
            success: true,
            expected_profit: Some(profit),
            expected_gas_used: Some(gas_used),
            error: None,
            simulation_time_ms: 0, // Will be set by the caller
            timestamp: chrono::Utc::now(),
        };

        Ok(result)
    }
}

#[async_trait]
impl Simulator for TenderlySimulator {
    async fn simulate(
        &self,
        opportunity: &ArbitrageOpportunity,
    ) -> Result<SimulationResult, ArbitrageError> {
        let start_time = Instant::now();

        // Create the simulation request
        let request = self.create_simulation_request(opportunity)?;

        // Send the simulation request
        let response = self.send_simulation_request(request).await?;

        // Parse the simulation response
        let mut result = self.parse_simulation_response(response, opportunity)?;

        // Set the simulation time
        result.simulation_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(result)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn simulator_type(&self) -> SimulatorType {
        SimulatorType::Tenderly
    }
}

/// Local simulator configuration
#[derive(Debug, Clone)]
pub struct LocalSimulatorConfig {
    pub networks: HashMap<Network, String>, // Network -> RPC URL
    pub dexes: HashMap<Address, DexInfo>,
    pub tokens: HashMap<Address, TokenInfo>,
    pub gas_price: U256,
    pub timeout_ms: u64,
}

/// Local simulator for simulating arbitrage opportunities using a local Ethereum node
pub struct LocalSimulator {
    config: LocalSimulatorConfig,
    providers: HashMap<Network, Arc<Provider<Http>>>,
    name: String,
}

impl LocalSimulator {
    /// Create a new local simulator
    pub fn new(config: LocalSimulatorConfig) -> Self {
        let name = "local_simulator".to_string();
        let providers = HashMap::new();

        Self {
            config,
            providers,
            name,
        }
    }

    /// Get or create a provider for a network
    async fn get_provider(
        &mut self,
        network: Network,
    ) -> Result<Arc<Provider<Http>>, ArbitrageError> {
        // Check if we already have a provider for this network
        if let Some(provider) = self.providers.get(&network) {
            return Ok(provider.clone());
        }

        // Get the RPC URL for this network
        let rpc_url = self.config.networks.get(&network).ok_or_else(|| {
            ArbitrageError::ConfigurationError(format!("No RPC URL found for {}", network))
        })?;

        // Create a new provider
        let provider = Arc::new(
            Provider::<Http>::try_from(rpc_url.as_str()).map_err(|e| {
                ArbitrageError::NetworkError(format!("Failed to create HTTP provider: {}", e))
            })?,
        );

        // Store the provider
        self.providers.insert(network, provider.clone());

        Ok(provider)
    }

    /// Simulate a swap on a DEX
    async fn simulate_swap(
        &mut self,
        opportunity: &ArbitrageOpportunity,
    ) -> Result<SimulationResult, ArbitrageError> {
        // Get the provider for this network
        let provider = self.get_provider(opportunity.network).await?;

        // Get the first token in the path
        let token_in = opportunity
            .token_path
            .first()
            .ok_or_else(|| ArbitrageError::SimulationError("Empty token path".to_string()))?;

        // Get the last token in the path
        let token_out = opportunity
            .token_path
            .last()
            .ok_or_else(|| ArbitrageError::SimulationError("Empty token path".to_string()))?;

        // Get the first DEX in the path
        let dex = opportunity
            .dex_path
            .first()
            .ok_or_else(|| ArbitrageError::SimulationError("Empty DEX path".to_string()))?;

        // Get DEX info
        let dex_info = self.config.dexes.get(dex).ok_or_else(|| {
            ArbitrageError::ConfigurationError(format!("No DEX info found for {}", dex))
        })?;

        // Create a contract instance for the DEX
        let dex_contract = Contract::new(
            dex_info.router_address,
            // This is a simplified example - in a real implementation, you would use the correct ABI for the DEX
            include_bytes!("../abis/UniswapV2Router02.json").to_vec(),
            provider.clone(),
        );

        // Create a call request for the swap
        let call_request = match dex_info.dex_type {
            DexType::UniswapV2 => {
                // Example: getAmountsOut(amountIn, path)
                let amount_in = opportunity.expected_profit;
                let path = opportunity.token_path.clone();

                dex_contract
                    .method::<_, Vec<U256>>("getAmountsOut", (amount_in, path))
                    .map_err(|e| {
                        ArbitrageError::SimulationError(format!(
                            "Failed to create getAmountsOut method: {}",
                            e
                        ))
                    })?
            }
            DexType::UniswapV3 => {
                // Example: quoteExactInputSingle(tokenIn, tokenOut, fee, amountIn, sqrtPriceLimitX96)
                let amount_in = opportunity.expected_profit;
                let fee = 3000u32; // Default fee tier (0.3%)
                let sqrt_price_limit_x96 = U256::zero(); // No price limit for simulation

                dex_contract
                    .method::<_, U256>(
                        "quoteExactInputSingle",
                        (token_in, token_out, fee, amount_in, sqrt_price_limit_x96),
                    )
                    .map_err(|e| {
                        ArbitrageError::SimulationError(format!(
                            "Failed to create quoteExactInputSingle method: {}",
                            e
                        ))
                    })?
            }
            DexType::Curve => {
                // Example: get_dy(i, j, dx)
                let i = 0i128; // Index of input token (simplified)
                let j = 1i128; // Index of output token (simplified)
                let dx = opportunity.expected_profit;

                dex_contract
                    .method::<_, U256>("get_dy", (i, j, dx))
                    .map_err(|e| {
                        ArbitrageError::SimulationError(format!(
                            "Failed to create get_dy method: {}",
                            e
                        ))
                    })?
            }
            _ => {
                return Err(ArbitrageError::SimulationError(format!(
                    "Unsupported DEX type: {:?}",
                    dex_info.dex_type
                )))
            }
        };

        // Call the contract to get the expected output amount
        let output_amount = match dex_info.dex_type {
            DexType::UniswapV2 => {
                let amounts: Vec<U256> = call_request.call().await.map_err(|e| {
                    ArbitrageError::SimulationError(format!("Failed to call getAmountsOut: {}", e))
                })?;
                *amounts.last().unwrap_or(&U256::zero())
            }
            DexType::UniswapV3 | DexType::Curve => {
                call_request.call().await.map_err(|e| {
                    ArbitrageError::SimulationError(format!("Failed to call quote method: {}", e))
                })?
            }
            _ => U256::zero(),
        };

        // Calculate the profit
        let profit = if output_amount > opportunity.expected_profit {
            output_amount - opportunity.expected_profit
        } else {
            U256::zero()
        };

        // Estimate the gas used
        let gas_used = U256::from(500000); // Simplified gas estimation

        // Create the simulation result
        let result = SimulationResult {
            opportunity_id: opportunity.id.clone(),
            success: true,
            expected_profit: Some(profit),
            expected_gas_used: Some(gas_used),
            error: None,
            simulation_time_ms: 0, // Will be set by the caller
            timestamp: chrono::Utc::now(),
        };

        Ok(result)
    }
}

#[async_trait]
impl Simulator for LocalSimulator {
    async fn simulate(
        &self,
        opportunity: &ArbitrageOpportunity,
    ) -> Result<SimulationResult, ArbitrageError> {
        let start_time = Instant::now();

        // Create a mutable copy of self to use in simulate_swap
        let mut simulator = LocalSimulator::new(self.config.clone());

        // Simulate the swap
        let mut result = simulator.simulate_swap(opportunity).await?;

        // Set the simulation time
        result.simulation_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(result)
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn simulator_type(&self) -> SimulatorType {
        SimulatorType::Local
    }
}
