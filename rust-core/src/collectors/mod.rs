pub mod mempool;
pub mod onchain;
pub mod price;

use crate::common::{AppState, ArbitrageResult, Network};
use async_trait::async_trait;
use ethers::prelude::*;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{debug, error, info, warn};

/// Data structure for opportunity data collected from various sources
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpportunityData {
    /// The network where the opportunity was found
    pub network: Network,
    /// The source of the opportunity (e.g., "mempool", "onchain", "price")
    pub source: String,
    /// The timestamp when the opportunity was found
    pub timestamp: u64,
    /// The raw data associated with the opportunity
    pub data: serde_json::Value,
}

/// Trait for collectors
#[async_trait]
pub trait Collector: Send + Sync + Debug {
    /// Get the name of the collector
    fn name(&self) -> &str;

    /// Get the network the collector is monitoring
    fn network(&self) -> Network;

    /// Start the collector
    async fn start(&mut self) -> ArbitrageResult<()>;

    /// Stop the collector
    async fn stop(&mut self) -> ArbitrageResult<()>;

    /// Collect data
    async fn collect(&self) -> ArbitrageResult<Vec<OpportunityData>>;
}

/// Factory for creating collectors
pub struct CollectorFactory;

impl CollectorFactory {
    /// Create a price collector
    pub fn create_price_collector(
        app_state: Arc<AppState>,
        network: Network,
        interval: Duration,
    ) -> Box<dyn Collector> {
        Box::new(price::PriceCollector::new(app_state, network, interval))
    }

    /// Create a mempool collector
    pub fn create_mempool_collector(
        app_state: Arc<AppState>,
        network: Network,
        interval: Duration,
    ) -> Box<dyn Collector> {
        Box::new(mempool::MempoolCollector::new(app_state, network, interval))
    }

    /// Create an on-chain collector
    pub fn create_onchain_collector(
        app_state: Arc<AppState>,
        network: Network,
        interval: Duration,
    ) -> Box<dyn Collector> {
        Box::new(onchain::OnchainCollector::new(app_state, network, interval))
    }
}

/// Manager for collectors
pub struct CollectorManager {
    app_state: Arc<AppState>,
    collectors: Vec<Box<dyn Collector>>,
    tx: Sender<OpportunityData>,
    rx: Option<Receiver<OpportunityData>>,
    handles: Vec<JoinHandle<()>>,
    running: bool,
}

impl CollectorManager {
    /// Create a new collector manager
    pub fn new(app_state: Arc<AppState>, tx: Sender<OpportunityData>) -> Self {
        Self {
            app_state,
            collectors: Vec::new(),
            tx,
            rx: None,
            handles: Vec::new(),
            running: false,
        }
    }

    /// Add a collector
    pub fn add_collector(&mut self, collector: Box<dyn Collector>) {
        info!(
            "Adding collector: {} for network {}",
            collector.name(),
            collector.network().name()
        );
        self.collectors.push(collector);
    }

    /// Start all collectors
    pub async fn start(&mut self) -> ArbitrageResult<()> {
        if self.running {
            warn!("Collector manager is already running");
            return Ok(());
        }

        info!("Starting collector manager with {} collectors", self.collectors.len());

        // Start each collector
        for collector in &mut self.collectors {
            info!(
                "Starting collector: {} for network {}",
                collector.name(),
                collector.network().name()
            );
            collector.start().await?;

            // Spawn a task for each collector
            let collector_clone = collector.clone();
            let tx = self.tx.clone();
            let handle = tokio::spawn(async move {
                Self::run_collector(collector_clone, tx).await;
            });
            self.handles.push(handle);
        }

        self.running = true;
        Ok(())
    }

    /// Run a collector in a loop
    async fn run_collector(collector: Box<dyn Collector>, tx: Sender<OpportunityData>) {
        info!(
            "Collector task started: {} for network {}",
            collector.name(),
            collector.network().name()
        );

        loop {
            match collector.collect().await {
                Ok(opportunities) => {
                    for opportunity in opportunities {
                        if let Err(e) = tx.send(opportunity).await {
                            error!("Failed to send opportunity: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!(
                        "Error collecting data from {}: {}",
                        collector.name(),
                        e
                    );
                }
            }

            // Sleep for a short time to avoid busy waiting
            time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Stop all collectors
    pub async fn stop(&mut self) -> ArbitrageResult<()> {
        if !self.running {
            warn!("Collector manager is not running");
            return Ok(());
        }

        info!("Stopping collector manager");

        // Stop each collector
        for collector in &mut self.collectors {
            info!(
                "Stopping collector: {} for network {}",
                collector.name(),
                collector.network().name()
            );
            collector.stop().await?;
        }

        // Abort all tasks
        for handle in self.handles.drain(..) {
            handle.abort();
        }

        self.running = false;
        Ok(())
    }

    /// Get the receiver for opportunities
    pub fn get_receiver(&self) -> Receiver<OpportunityData> {
        self.rx.clone().expect("Receiver not initialized")
    }
}
