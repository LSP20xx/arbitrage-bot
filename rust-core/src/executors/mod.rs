mod flashbots;
mod standard;

pub use flashbots::*;
pub use standard::*;

use crate::common::{
    AppState, ArbitrageError, ArbitrageOpportunity, ArbitrageResult, ExecutionResult, Network,
    FlashbotsConfig,
};
use async_trait::async_trait;
use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Execution event types
#[derive(Debug, Clone)]
pub enum ExecutionEvent {
    /// Execution started
    Started(ArbitrageOpportunity),
    /// Execution completed
    Completed(ExecutionResult),
    /// Error event
    Error(String),
}

/// Executor trait for arbitrage execution components
#[async_trait]
pub trait Executor: Send + Sync {
    /// Execute an arbitrage opportunity
    async fn execute(&self, opportunity: ArbitrageOpportunity) -> ArbitrageResult<ExecutionResult>;

    /// Get the executor name
    fn name(&self) -> &str;
}

/// Factory for creating executors
pub struct ExecutorFactory;

impl ExecutorFactory {
    /// Create a new Flashbots executor
    pub fn create_flashbots_executor(
        app_state: Arc<AppState>,
        config: FlashbotsConfig,
    ) -> Arc<dyn Executor> {
        Arc::new(FlashbotsExecutor::new(app_state, config))
    }

    /// Create a new standard executor
    pub fn create_standard_executor(
        app_state: Arc<AppState>,
    ) -> Arc<dyn Executor> {
        Arc::new(StandardExecutor::new(app_state))
    }
}

/// Execution manager
pub struct ExecutionManager {
    app_state: Arc<AppState>,
    executors: Vec<Arc<dyn Executor>>,
    tx: mpsc::Sender<ExecutionEvent>,
}

impl ExecutionManager {
    /// Create a new execution manager
    pub fn new(
        app_state: Arc<AppState>,
        tx: mpsc::Sender<ExecutionEvent>,
    ) -> Self {
        Self {
            app_state,
            executors: Vec::new(),
            tx,
        }
    }

    /// Add an executor
    pub fn add_executor(&mut self, executor: Arc<dyn Executor>) {
        self.executors.push(executor);
    }

    /// Execute an arbitrage opportunity
    pub async fn execute(&self, opportunity: ArbitrageOpportunity) -> ArbitrageResult<ExecutionResult> {
        if self.executors.is_empty() {
            return Err(ArbitrageError::ExecutionError(
                "No executors available".to_string(),
            ));
        }

        // Notify that execution has started
        if let Err(e) = self.tx.send(ExecutionEvent::Started(opportunity.clone())).await {
            error!("Failed to send execution started event: {}", e);
        }

        // Choose the appropriate executor based on the opportunity
        // In a real implementation, you would have more sophisticated logic here
        let executor = &self.executors[0];

        debug!(
            "Executing opportunity {} with executor {}",
            opportunity.id,
            executor.name()
        );

        // Execute the opportunity
        match executor.execute(opportunity.clone()).await {
            Ok(result) => {
                // Notify that execution has completed
                if let Err(e) = self.tx.send(ExecutionEvent::Completed(result.clone())).await {
                    error!("Failed to send execution completed event: {}", e);
                }

                Ok(result)
            }
            Err(e) => {
                let error = format!(
                    "Executor {} failed to execute opportunity {}: {}",
                    executor.name(),
                    opportunity.id,
                    e
                );

                // Notify of the error
                if let Err(e) = self.tx.send(ExecutionEvent::Error(error.clone())).await {
                    error!("Failed to send execution error event: {}", e);
                }

                Err(ArbitrageError::ExecutionError(error))
            }
        }
    }
}
