use crate::common::{AppState, ArbitrageError, ArbitrageOpportunity, ArbitrageResult, Network};
use async_trait::async_trait;
use ethers::prelude::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Tenderly simulation request
#[derive(Debug, Clone, Serialize)]
struct TenderlySimulationRequest {
    network_id: String,
    from: String,
    to: String,
    input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    gas: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gas_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    save: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    save_if_fails: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    state_objects: Option<serde_json::Value>,
}

/// Tenderly simulation response
#[derive(Debug, Clone, Deserialize)]
struct TenderlySimulationResponse {
    transaction: TenderlyTransaction,
    simulation: TenderlySimulationResult,
}

/// Tenderly transaction
#[derive(Debug, Clone, Deserialize)]
struct TenderlyTransaction {
    hash: String,
    from: String,
    to: String,
    gas: u64,
    gas_price: String,
    value: String,
}

/// Tenderly simulation result
#[derive(Debug, Clone, Deserialize)]
struct TenderlySimulationResult {
    id: String,
    status: bool,
    gas_used: u64,
    error_message: Option<String>,
    call_trace: Option<serde_json::Value>,
}

/// Tenderly simulator for arbitrage simulation
pub struct TenderlySimulator {
    api_key: String,
    account_id: String,
    project_slug: String,
    app_state: Arc<AppState>,
    http_client: Client,
}

impl TenderlySimulator {
    /// Create a new Tenderly simulator
    pub fn new(
        api_key: String,
        account_id: String,
        project_slug: String,
        app_state: Arc<AppState>,
    ) -> Self {
        Self {
            api_key,
            account_id,
            project_slug,
            app_state,
            http_client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }

    /// Build the simulation URL
    fn build_simulation_url(&self) -> String {
        format!(
            "https://api.tenderly.co/api/v1/account/{}/project/{}/simulate",
            self.account_id, self.project_slug
        )
    }

    /// Simulate a transaction using Tenderly
    async fn simulate_transaction(
        &self,
        network_id: String,
        from: Address,
        to: Address,
        input: Bytes,
        gas: Option<u64>,
        gas_price: Option<U256>,
        value: Option<U256>,
    ) -> ArbitrageResult<TenderlySimulationResponse> {
        debug!("Simulating transaction on Tenderly");

        // Build the request
        let request = TenderlySimulationRequest {
            network_id,
            from: format!("{:?}", from),
            to: format!("{:?}", to),
            input: format!("0x{}", hex::encode(input.as_ref())),
            gas,
            gas_price: gas_price.map(|p| format!("{:?}", p)),
            value: value.map(|v| format!("{:?}", v)),
            save: Some(false),
            save_if_fails: Some(true),
            state_objects: None,
        };

        // Build the URL
        let url = self.build_simulation_url();

        // Make the request
        let response = self
            .http_client
            .post(&url)
            .header("X-Access-Key", &self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                ArbitrageError::SimulationError(format!("Failed to send Tenderly request: {}", e))
            })?;

        // Check the response status
        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(ArbitrageError::SimulationError(format!(
                "Tenderly API error: {} - {}",
                response.status(),
                error_text
            )));
        }

        // Parse the response
        let simulation_response: TenderlySimulationResponse = response.json().await.map_err(|e| {
            ArbitrageError::SimulationError(format!("Failed to parse Tenderly response: {}", e))
        })?;

        Ok(simulation_response)
    }
}

#[async_trait]
impl super::Simulator for TenderlySimulator {
    async fn simulate(
        &self,
        opportunity: ArbitrageOpportunity,
    ) -> ArbitrageResult<super::SimulationResult> {
        debug!(
            "Simulating arbitrage opportunity {} with Tenderly",
            opportunity.id
        );

        // Get the network ID
        let network_id = match opportunity.network {
            Network::Mainnet => "1",
            Network::Arbitrum => "42161",
            Network::Optimism => "10",
            Network::Polygon => "137",
            Network::Base => "8453",
            Network::Custom(id) => return Err(ArbitrageError::SimulationError(format!(
                "Custom network ID {} not supported by Tenderly simulator",
                id
            ))),
        }
        .to_string();

        // Get the arbitrage executor contract address
        let executor_address = match self.app_state.providers.get(&opportunity.network) {
            Some(provider) => {
                // In a real implementation, you would get this from the app state or config
                // For now, we'll use a placeholder
                Address::from_slice(&[0; 20])
            }
            None => {
                return Err(ArbitrageError::SimulationError(format!(
                    "No provider available for network {}",
                    opportunity.network.name()
                )));
            }
        };

        // Encode the arbitrage execution call
        // In a real implementation, you would encode the actual call to the arbitrage executor
        // For now, we'll use a placeholder
        let input = Bytes::from(vec![0; 100]);

        // Simulate the transaction
        let simulation_response = self
            .simulate_transaction(
                network_id,
                Address::from_slice(&[0; 20]), // Sender address (placeholder)
                executor_address,
                input,
                Some(3000000), // Gas limit
                None,          // Gas price (use network default)
                None,          // Value (no ETH sent)
            )
            .await?;

        // Check if the simulation was successful
        let success = simulation_response.simulation.status;
        let gas_used = U256::from(simulation_response.simulation.gas_used);
        let error = simulation_response.simulation.error_message;
        let trace = simulation_response
            .simulation
            .call_trace
            .map(|t| serde_json::to_string(&t).unwrap_or_default());

        // Create the simulation result
        let result = super::SimulationResult {
            opportunity: opportunity.clone(),
            success,
            actual_output: if success {
                Some(opportunity.expected_output)
            } else {
                None
            },
            gas_used: Some(gas_used),
            error,
            trace,
        };

        Ok(result)
    }

    fn name(&self) -> &str {
        "TenderlySimulator"
    }
}
