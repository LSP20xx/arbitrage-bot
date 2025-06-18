use crate::collectors::{Collector, CollectorManager, OpportunityData};
use crate::common::{AppState, ArbitrageResult};
use crate::executors::{ExecutionManager, ExecutionResult};
use crate::simulators::{SimulationManager, SimulationResult};
use crate::strategies::{Strategy, StrategyManager, StrategyResult};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time;
use tracing::{debug, error, info, warn};

/// Configuration for the orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Maximum number of concurrent strategies
    pub max_concurrent_strategies: usize,
    /// Maximum number of concurrent simulations
    pub max_concurrent_simulations: usize,
    /// Maximum number of concurrent executions
    pub max_concurrent_executions: usize,
    /// Timeout for strategy evaluation
    pub strategy_timeout: Duration,
    /// Timeout for simulation
    pub simulation_timeout: Duration,
    /// Timeout for execution
    pub execution_timeout: Duration,
    /// Minimum profit threshold in USD
    pub min_profit_threshold_usd: f64,
    /// Circuit breaker threshold (maximum consecutive failures)
    pub circuit_breaker_threshold: usize,
    /// Circuit breaker reset time
    pub circuit_breaker_reset_time: Duration,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_concurrent_strategies: 10,
            max_concurrent_simulations: 5,
            max_concurrent_executions: 2,
            strategy_timeout: Duration::from_secs(5),
            simulation_timeout: Duration::from_secs(10),
            execution_timeout: Duration::from_secs(30),
            min_profit_threshold_usd: 10.0,
            circuit_breaker_threshold: 5,
            circuit_breaker_reset_time: Duration::from_secs(300),
        }
    }
}

/// The orchestrator coordinates all components of the arbitrage bot
pub struct Orchestrator {
    app_state: Arc<AppState>,
    config: OrchestratorConfig,
    collector_manager: CollectorManager,
    strategy_manager: StrategyManager,
    simulation_manager: SimulationManager,
    execution_manager: ExecutionManager,
    consecutive_failures: usize,
    circuit_breaker_active: bool,
}

impl Orchestrator {
    /// Create a new orchestrator
    pub fn new(
        app_state: Arc<AppState>,
        config: OrchestratorConfig,
        collector_manager: CollectorManager,
        strategy_manager: StrategyManager,
        simulation_manager: SimulationManager,
        execution_manager: ExecutionManager,
    ) -> Self {
        Self {
            app_state,
            config,
            collector_manager,
            strategy_manager,
            simulation_manager,
            execution_manager,
            consecutive_failures: 0,
            circuit_breaker_active: false,
        }
    }

    /// Start the orchestrator
    pub async fn start(&mut self) -> ArbitrageResult<()> {
        info!("Starting the orchestrator");

        // Start the collector manager
        self.collector_manager.start().await?;

        // Start the strategy manager
        self.strategy_manager.start().await?;

        // Start the simulation manager
        self.simulation_manager.start().await?;

        // Start the execution manager
        self.execution_manager.start().await?;

        // Start the main loop
        self.run_main_loop().await?;

        Ok(())
    }

    /// Run the main orchestration loop
    async fn run_main_loop(&mut self) -> ArbitrageResult<()> {
        info!("Starting the main orchestration loop");

        // Get the channels
        let collector_rx = self.collector_manager.get_receiver();
        let strategy_rx = self.strategy_manager.get_receiver();
        let simulation_rx = self.simulation_manager.get_receiver();
        let execution_rx = self.execution_manager.get_receiver();

        // Process opportunities from collectors
        tokio::spawn(self.process_opportunities(collector_rx));

        // Process strategy results
        tokio::spawn(self.process_strategy_results(strategy_rx));

        // Process simulation results
        tokio::spawn(self.process_simulation_results(simulation_rx));

        // Process execution results
        tokio::spawn(self.process_execution_results(execution_rx));

        // Keep the main thread alive
        loop {
            // Check circuit breaker
            if self.circuit_breaker_active {
                warn!("Circuit breaker is active, waiting before resuming operations");
                time::sleep(self.config.circuit_breaker_reset_time).await;
                self.reset_circuit_breaker();
            }

            // Sleep to avoid busy waiting
            time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Process opportunities from collectors
    async fn process_opportunities(
        &self,
        mut rx: Receiver<OpportunityData>,
    ) -> ArbitrageResult<()> {
        info!("Starting to process opportunities from collectors");

        while let Some(opportunity) = rx.recv().await {
            if self.circuit_breaker_active {
                debug!("Circuit breaker active, skipping opportunity");
                continue;
            }

            debug!("Received opportunity from collector: {:?}", opportunity);

            // Send to strategy manager for evaluation
            self.strategy_manager.evaluate_opportunity(opportunity).await?;
        }

        Ok(())
    }

    /// Process strategy results
    async fn process_strategy_results(
        &self,
        mut rx: Receiver<StrategyResult>,
    ) -> ArbitrageResult<()> {
        info!("Starting to process strategy results");

        while let Some(result) = rx.recv().await {
            if self.circuit_breaker_active {
                debug!("Circuit breaker active, skipping strategy result");
                continue;
            }

            match result {
                StrategyResult::Opportunity(opportunity) => {
                    debug!("Received strategy opportunity: {:?}", opportunity);

                    // Check if the profit is above the threshold
                    if opportunity.estimated_profit_usd < self.config.min_profit_threshold_usd {
                        debug!(
                            "Opportunity profit ${} below threshold ${}",
                            opportunity.estimated_profit_usd, self.config.min_profit_threshold_usd
                        );
                        continue;
                    }

                    // Send to simulation manager
                    self.simulation_manager.simulate(opportunity).await?;
                }
                StrategyResult::Error(err) => {
                    warn!("Strategy error: {:?}", err);
                }
            }
        }

        Ok(())
    }

    /// Process simulation results
    async fn process_simulation_results(
        &self,
        mut rx: Receiver<SimulationResult>,
    ) -> ArbitrageResult<()> {
        info!("Starting to process simulation results");

        while let Some(result) = rx.recv().await {
            if self.circuit_breaker_active {
                debug!("Circuit breaker active, skipping simulation result");
                continue;
            }

            match result {
                SimulationResult::Success(opportunity) => {
                    info!(
                        "Simulation successful for opportunity with profit ${}",
                        opportunity.estimated_profit_usd
                    );

                    // Send to execution manager
                    self.execution_manager.execute(opportunity).await?;
                }
                SimulationResult::Failure(opportunity, error) => {
                    warn!(
                        "Simulation failed for opportunity with profit ${}: {:?}",
                        opportunity.estimated_profit_usd, error
                    );
                }
            }
        }

        Ok(())
    }

    /// Process execution results
    async fn process_execution_results(
        &mut self,
        mut rx: Receiver<ExecutionResult>,
    ) -> ArbitrageResult<()> {
        info!("Starting to process execution results");

        while let Some(result) = rx.recv().await {
            match result {
                ExecutionResult::Success(opportunity, tx_hash) => {
                    info!(
                        "Execution successful for opportunity with profit ${}, tx hash: {}",
                        opportunity.estimated_profit_usd, tx_hash
                    );

                    // Reset consecutive failures
                    self.consecutive_failures = 0;
                }
                ExecutionResult::Failure(opportunity, error) => {
                    error!(
                        "Execution failed for opportunity with profit ${}: {:?}",
                        opportunity.estimated_profit_usd, error
                    );

                    // Increment consecutive failures
                    self.consecutive_failures += 1;

                    // Check if circuit breaker should be activated
                    if self.consecutive_failures >= self.config.circuit_breaker_threshold {
                        self.activate_circuit_breaker();
                    }
                }
            }
        }

        Ok(())
    }

    /// Activate the circuit breaker
    fn activate_circuit_breaker(&mut self) {
        warn!(
            "Activating circuit breaker after {} consecutive failures",
            self.consecutive_failures
        );
        self.circuit_breaker_active = true;
    }

    /// Reset the circuit breaker
    fn reset_circuit_breaker(&mut self) {
        info!("Resetting circuit breaker");
        self.circuit_breaker_active = false;
        self.consecutive_failures = 0;
    }

    /// Stop the orchestrator
    pub async fn stop(&mut self) -> ArbitrageResult<()> {
        info!("Stopping the orchestrator");

        // Stop the collector manager
        self.collector_manager.stop().await?;

        // Stop the strategy manager
        self.strategy_manager.stop().await?;

        // Stop the simulation manager
        self.simulation_manager.stop().await?;

        // Stop the execution manager
        self.execution_manager.stop().await?;

        Ok(())
    }
}
