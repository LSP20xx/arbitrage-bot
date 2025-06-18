mod tenderly;
mod local;
mod forked;

pub use tenderly::*;
pub use local::*;
pub use forked::*;

use crate::common::{
    AppState, ArbitrageError, ArbitrageOpportunity, ArbitrageResult, ExecutionResult, Network,
    SwapStep, Token,
};
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Simulation result
#[derive(Debug, Clone)]
pub struct SimulationResult {
    /// The opportunity that was simulated
    pub opportunity: ArbitrageOpportunity,
    /// Whether the simulation was successful
    pub success: bool,
    /// The actual output amount
    pub actual_output: Option<U256>,
    /// The gas used
    pub gas_used: Option<U256>,
    /// The error message if the simulation failed
    pub error: Option<String>,
    /// The transaction trace if available
    pub trace: Option<String>,
}

/// Simulator trait for arbitrage simulation components
#[async_trait]
pub trait Simulator: Send + Sync {
    /// Simulate an arbitrage opportunity
    async fn simulate(&self, opportunity: ArbitrageOpportunity) -> ArbitrageResult<SimulationResult>;

    /// Get the simulator name
    fn name(&self) -> &str;
}

/// Factory for creating simulators
pub struct SimulatorFactory;

impl SimulatorFactory {
    /// Create a new Tenderly simulator
    pub fn create_tenderly_simulator(
        api_key: String,
        account_id: String,
        project_slug: String,
        app_state: Arc<AppState>,
    ) -> Arc<dyn Simulator> {
        Arc::new(TenderlySimulator::new(api_key, account_id, project_slug, app_state))
    }

    /// Create a new local simulator
    pub fn create_local_simulator(
        app_state: Arc<AppState>,
    ) -> Arc<dyn Simulator> {
        Arc::new(LocalSimulator::new(app_state))
    }

    /// Create a new forked simulator
    pub fn create_forked_simulator(
        app_state: Arc<AppState>,
        fork_url: String,
        fork_block_number: Option<u64>,
    ) -> Arc<dyn Simulator> {
        Arc::new(ForkedSimulator::new(app_state, fork_url, fork_block_number))
    }
}

/// Simulation event types
#[derive(Debug, Clone)]
pub enum SimulationEvent {
    /// Simulation started
    Started(ArbitrageOpportunity),
    /// Simulation completed
    Completed(SimulationResult),
    /// Error event
    Error(String),
}

/// Simulation manager
pub struct SimulationManager {
    app_state: Arc<AppState>,
    simulators: Vec<Arc<dyn Simulator>>,
    tx: mpsc::Sender<SimulationEvent>,
}

impl SimulationManager {
    /// Create a new simulation manager
    pub fn new(
        app_state: Arc<AppState>,
        tx: mpsc::Sender<SimulationEvent>,
    ) -> Self {
        Self {
            app_state,
            simulators: Vec::new(),
            tx,
        }
    }

    /// Add a simulator
    pub fn add_simulator(&mut self, simulator: Arc<dyn Simulator>) {
        self.simulators.push(simulator);
    }

    /// Simulate an arbitrage opportunity
    pub async fn simulate(&self, opportunity: ArbitrageOpportunity) -> ArbitrageResult<SimulationResult> {
        if self.simulators.is_empty() {
            return Err(ArbitrageError::SimulationError(
                "No simulators available".to_string(),
            ));
        }

        // Notify that simulation has started
        if let Err(e) = self.tx.send(SimulationEvent::Started(opportunity.clone())).await {
            error!("Failed to send simulation started event: {}", e);
        }

        // Try each simulator in order until one succeeds
        for simulator in &self.simulators {
            debug!(
                "Simulating opportunity {} with simulator {}",
                opportunity.id,
                simulator.name()
            );

            match simulator.simulate(opportunity.clone()).await {
                Ok(result) => {
                    // Notify that simulation has completed
                    if let Err(e) = self.tx.send(SimulationEvent::Completed(result.clone())).await {
                        error!("Failed to send simulation completed event: {}", e);
                    }

                    return Ok(result);
                }
                Err(e) => {
                    warn!(
                        "Simulator {} failed to simulate opportunity {}: {}",
                        simulator.name(),
                        opportunity.id,
                        e
                    );

                    // Try the next simulator
                    continue;
                }
            }
        }

        // All simulators failed
        let error = format!(
            "All simulators failed to simulate opportunity {}",
            opportunity.id
        );

        // Notify of the error
        if let Err(e) = self.tx.send(SimulationEvent::Error(error.clone())).await {
            error!("Failed to send simulation error event: {}", e);
        }

        Err(ArbitrageError::SimulationError(error))
    }
}
