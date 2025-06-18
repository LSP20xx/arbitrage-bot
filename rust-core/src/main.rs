mod collectors;
mod common;
mod executors;
mod orchestrator;
mod simulators;
mod strategies;

use crate::collectors::{CollectorFactory, CollectorManager};
use crate::common::{AppState, ArbitrageResult, Config, Network};
use crate::executors::{ExecutorFactory, ExecutionManager};
use crate::orchestrator::{Orchestrator, OrchestratorConfig};
use crate::simulators::{SimulatorFactory, SimulationManager};
use crate::strategies::{StrategyFactory, StrategyManager};
use clap::{App, Arg, SubCommand};
use ethers::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

/// Initialize the logger
fn init_logger(level: Level) {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set logger");
}

/// Load the configuration
async fn load_config(config_path: &str) -> ArbitrageResult<Config> {
    info!("Loading configuration from {}", config_path);
    let config = Config::from_file(config_path).await?;
    Ok(config)
}

/// Initialize the application state
async fn init_app_state(config: Config) -> ArbitrageResult<Arc<AppState>> {
    info!("Initializing application state");
    let app_state = AppState::new(config);
    let app_state = Arc::new(app_state);
    Ok(app_state)
}

/// Run the arbitrage bot
async fn run_bot(config_path: &str, log_level: Level) -> ArbitrageResult<()> {
    // Initialize the logger
    init_logger(log_level);

    // Load the configuration
    let config = load_config(config_path).await?;

    // Initialize the application state
    let app_state = init_app_state(config).await?;

    // Initialize providers
    for (network, rpc_url) in &app_state.config.rpc_urls {
        info!("Connecting to network: {}", network.name());
        let provider = Provider::<Ws>::connect(rpc_url).await?;
        app_state.providers.insert(*network, Arc::new(provider));
    }

    // Create channels
    let (collector_tx, collector_rx) = mpsc::channel(100);
    let (strategy_tx, strategy_rx) = mpsc::channel(100);
    let (simulation_tx, simulation_rx) = mpsc::channel(100);
    let (execution_tx, execution_rx) = mpsc::channel(100);

    // Initialize the collector manager
    let mut collector_manager = CollectorManager::new(app_state.clone(), collector_tx);
    for network in app_state.config.networks.iter() {
        // Add collectors
        collector_manager.add_collector(
            CollectorFactory::create_price_collector(
                app_state.clone(),
                *network,
                Duration::from_secs(60),
            )
        );
        collector_manager.add_collector(
            CollectorFactory::create_mempool_collector(
                app_state.clone(),
                *network,
                Duration::from_secs(1),
            )
        );
        collector_manager.add_collector(
            CollectorFactory::create_onchain_collector(
                app_state.clone(),
                *network,
                Duration::from_secs(10),
            )
        );
    }

    // Initialize the strategy manager
    let mut strategy_manager = StrategyManager::new(app_state.clone(), strategy_tx);
    for network in app_state.config.networks.iter() {
        // Add strategies
        strategy_manager.add_strategy(
            StrategyFactory::create_triangular_strategy(
                app_state.clone(),
                *network,
                10.0, // $10 minimum profit
            )
        );
        strategy_manager.add_strategy(
            StrategyFactory::create_cross_exchange_strategy(
                app_state.clone(),
                *network,
                10.0, // $10 minimum profit
            )
        );
        strategy_manager.add_strategy(
            StrategyFactory::create_flash_loan_strategy(
                app_state.clone(),
                *network,
                10.0, // $10 minimum profit
            )
        );
    }

    // Add cross-chain strategies
    if app_state.config.networks.contains(&Network::Mainnet) && app_state.config.networks.contains(&Network::Arbitrum) {
        strategy_manager.add_strategy(
            StrategyFactory::create_cross_chain_strategy(
                app_state.clone(),
                Network::Mainnet,
                Network::Arbitrum,
                20.0, // $20 minimum profit (higher for cross-chain)
            )
        );
    }

    // Initialize the simulation manager
    let mut simulation_manager = SimulationManager::new(app_state.clone(), simulation_tx);
    for network in app_state.config.networks.iter() {
        // Add simulators
        simulation_manager.add_simulator(
            SimulatorFactory::create_local_simulator(app_state.clone())
        );
        simulation_manager.add_simulator(
            SimulatorFactory::create_tenderly_simulator(
                app_state.clone(),
                "your-tenderly-user".to_string(),
                "your-tenderly-project".to_string(),
                "your-tenderly-access-key".to_string(),
            )
        );
    }

    // Initialize the execution manager
    let mut execution_manager = ExecutionManager::new(app_state.clone(), execution_tx);
    for network in app_state.config.networks.iter() {
        // Add executors
        execution_manager.add_executor(
            ExecutorFactory::create_standard_executor(app_state.clone())
        );
        execution_manager.add_executor(
            ExecutorFactory::create_flashbots_executor(
                app_state.clone(),
                app_state.config.flashbots_config.clone(),
            )
        );
    }

    // Initialize the orchestrator
    let orchestrator_config = OrchestratorConfig::default();
    let mut orchestrator = Orchestrator::new(
        app_state.clone(),
        orchestrator_config,
        collector_manager,
        strategy_manager,
        simulation_manager,
        execution_manager,
    );

    // Start the orchestrator
    orchestrator.start().await?;

    Ok(())
}

#[tokio::main]
async fn main() -> ArbitrageResult<()> {
    // Parse command line arguments
    let matches = App::new("DeFi Arbitrage Bot")
        .version("1.0")
        .author("Your Name <your.email@example.com>")
        .about("High-performance DeFi arbitrage bot")
        .arg(
            Arg::with_name("config")
                .short("c")
                .long("config")
                .value_name("FILE")
                .help("Sets a custom config file")
                .default_value("config.json5")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("log-level")
                .short("l")
                .long("log-level")
                .value_name("LEVEL")
                .help("Sets the log level (trace, debug, info, warn, error)")
                .default_value("info")
                .takes_value(true),
        )
        .get_matches();

    // Get the config file path
    let config_path = matches.value_of("config").unwrap();

    // Get the log level
    let log_level = match matches.value_of("log-level").unwrap() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    // Run the bot
    run_bot(config_path, log_level).await
}
