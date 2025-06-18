mod triangular;
mod cross_exchange;
mod cross_chain;
mod flash_loan;

pub use triangular::*;
pub use cross_exchange::*;
pub use cross_chain::*;
pub use flash_loan::*;

use crate::common::{
    AppState, ArbitrageError, ArbitrageOpportunity, ArbitrageResult, Network, Pool, SwapStep, Token,
};
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Strategy event types
#[derive(Debug, Clone)]
pub enum StrategyEvent {
    /// Opportunity found
    OpportunityFound(ArbitrageOpportunity),
    /// Error event
    Error(String),
}

/// Strategy trait for arbitrage strategy components
#[async_trait]
pub trait Strategy: Send + Sync {
    /// Find arbitrage opportunities
    async fn find_opportunities(&self) -> ArbitrageResult<Vec<ArbitrageOpportunity>>;

    /// Get the strategy name
    fn name(&self) -> &str;
}

/// Factory for creating strategies
pub struct StrategyFactory;

impl StrategyFactory {
    /// Create a new triangular arbitrage strategy
    pub fn create_triangular_strategy(
        app_state: Arc<AppState>,
        network: Network,
        min_profit_threshold: f64,
    ) -> Arc<dyn Strategy> {
        Arc::new(TriangularArbitrageStrategy::new(app_state, network, min_profit_threshold))
    }

    /// Create a new cross-exchange arbitrage strategy
    pub fn create_cross_exchange_strategy(
        app_state: Arc<AppState>,
        network: Network,
        min_profit_threshold: f64,
    ) -> Arc<dyn Strategy> {
        Arc::new(CrossExchangeArbitrageStrategy::new(app_state, network, min_profit_threshold))
    }

    /// Create a new cross-chain arbitrage strategy
    pub fn create_cross_chain_strategy(
        app_state: Arc<AppState>,
        source_network: Network,
        target_network: Network,
        min_profit_threshold: f64,
    ) -> Arc<dyn Strategy> {
        Arc::new(CrossChainArbitrageStrategy::new(
            app_state,
            source_network,
            target_network,
            min_profit_threshold,
        ))
    }

    /// Create a new flash loan arbitrage strategy
    pub fn create_flash_loan_strategy(
        app_state: Arc<AppState>,
        network: Network,
        min_profit_threshold: f64,
    ) -> Arc<dyn Strategy> {
        Arc::new(FlashLoanArbitrageStrategy::new(app_state, network, min_profit_threshold))
    }
}

/// Strategy manager
pub struct StrategyManager {
    app_state: Arc<AppState>,
    strategies: Vec<Arc<dyn Strategy>>,
    tx: mpsc::Sender<StrategyEvent>,
}

impl StrategyManager {
    /// Create a new strategy manager
    pub fn new(
        app_state: Arc<AppState>,
        tx: mpsc::Sender<StrategyEvent>,
    ) -> Self {
        Self {
            app_state,
            strategies: Vec::new(),
            tx,
        }
    }

    /// Add a strategy
    pub fn add_strategy(&mut self, strategy: Arc<dyn Strategy>) {
        self.strategies.push(strategy);
    }

    /// Find arbitrage opportunities
    pub async fn find_opportunities(&self) -> ArbitrageResult<Vec<ArbitrageOpportunity>> {
        if self.strategies.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_opportunities = Vec::new();

        // Run all strategies in parallel
        let mut tasks = Vec::new();
        for strategy in &self.strategies {
            let strategy = strategy.clone();
            let task = tokio::spawn(async move {
                match strategy.find_opportunities().await {
                    Ok(opportunities) => (opportunities, None),
                    Err(e) => (Vec::new(), Some(e)),
                }
            });
            tasks.push(task);
        }

        // Collect results
        for task in tasks {
            match task.await {
                Ok((opportunities, error)) => {
                    if let Some(e) = error {
                        error!("Strategy error: {}", e);
                        if let Err(e) = self
                            .tx
                            .send(StrategyEvent::Error(format!("Strategy error: {}", e)))
                            .await
                        {
                            error!("Failed to send strategy error event: {}", e);
                        }
                    } else {
                        for opportunity in &opportunities {
                            debug!(
                                "Found opportunity: {} with expected profit: {}",
                                opportunity.id, opportunity.expected_profit
                            );
                            if let Err(e) = self
                                .tx
                                .send(StrategyEvent::OpportunityFound(opportunity.clone()))
                                .await
                            {
                                error!("Failed to send opportunity found event: {}", e);
                            }
                        }
                        all_opportunities.extend(opportunities);
                    }
                }
                Err(e) => {
                    error!("Task error: {}", e);
                    if let Err(e) = self
                        .tx
                        .send(StrategyEvent::Error(format!("Task error: {}", e)))
                        .await
                    {
                        error!("Failed to send task error event: {}", e);
                    }
                }
            }
        }

        Ok(all_opportunities)
    }
}
