use ethers::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use thiserror::Error;
use tokio::sync::Mutex;

/// Supported networks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Network {
    Mainnet,
    Arbitrum,
    Optimism,
    Polygon,
    Base,
}

impl Network {
    /// Get the name of the network
    pub fn name(&self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Arbitrum => "arbitrum",
            Network::Optimism => "optimism",
            Network::Polygon => "polygon",
            Network::Base => "base",
        }
    }

    /// Get the chain ID of the network
    pub fn chain_id(&self) -> u64 {
        match self {
            Network::Mainnet => 1,
            Network::Arbitrum => 42161,
            Network::Optimism => 10,
            Network::Polygon => 137,
            Network::Base => 8453,
        }
    }

    /// Parse a network from a string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mainnet" => Some(Network::Mainnet),
            "arbitrum" => Some(Network::Arbitrum),
            "optimism" => Some(Network::Optimism),
            "polygon" => Some(Network::Polygon),
            "base" => Some(Network::Base),
            _ => None,
        }
    }
}

/// Flashbots configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlashbotsConfig {
    pub enabled: bool,
    pub relay_urls: Vec<String>,
    pub min_bid_percentage: f64,
    pub max_bid_percentage: Option<f64>,
    pub bundle_timeout: Option<u64>,
    pub max_blocks_to_search: Option<u64>,
}

/// Tenderly configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenderlyConfig {
    pub enabled: bool,
    pub user: String,
    pub project: String,
    pub access_key: String,
}

/// Strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    pub enabled: bool,
    pub min_profit_usd: f64,
    pub max_path_length: Option<usize>,
    pub interval: Option<u64>,
}

/// DEX configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DexConfig {
    pub enabled: bool,
    pub factory_address: String,
    pub router_address: Option<String>,
}

/// Token configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenConfig {
    pub mainnet: Option<String>,
    pub arbitrum: Option<String>,
    pub optimism: Option<String>,
    pub polygon: Option<String>,
    pub base: Option<String>,
}

impl TokenConfig {
    /// Get the address for a specific network
    pub fn address_for_network(&self, network: Network) -> Option<String> {
        match network {
            Network::Mainnet => self.mainnet.clone(),
            Network::Arbitrum => self.arbitrum.clone(),
            Network::Optimism => self.optimism.clone(),
            Network::Polygon => self.polygon.clone(),
            Network::Base => self.base.clone(),
        }
    }
}

/// Main configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub general: GeneralConfig,
    pub networks: Vec<Network>,
    pub rpc_urls: HashMap<Network, String>,
    pub wallet: WalletConfig,
    pub flashbots: FlashbotsConfig,
    pub tenderly: Option<TenderlyConfig>,
    pub strategies: StrategiesConfig,
    pub dexes: HashMap<String, DexConfig>,
    pub tokens: HashMap<String, TokenConfig>,
}

/// General configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub name: String,
    pub version: String,
    pub log_level: String,
    pub data_dir: String,
    pub metrics_enabled: Option<bool>,
    pub metrics_port: Option<u16>,
    pub api_enabled: Option<bool>,
    pub api_port: Option<u16>,
    pub api_host: Option<String>,
}

/// Wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    pub private_key: String,
    pub address: String,
    pub gas_limit_multiplier: Option<f64>,
    pub max_gas_price: Option<u64>,
    pub priority_fee: Option<u64>,
}

/// Strategies configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategiesConfig {
    pub triangular: StrategyConfig,
    pub cross_exchange: StrategyConfig,
    pub cross_chain: StrategyConfig,
    pub flash_loan: StrategyConfig,
}

/// Application state
pub struct AppState {
    pub config: Config,
    pub providers: RwLock<HashMap<Network, Arc<Provider<Ws>>>>,
    pub wallets: RwLock<HashMap<Network, Arc<SignerMiddleware<Arc<Provider<Ws>>, LocalWallet>>>>,
    pub price_cache: Mutex<HashMap<String, f64>>,
}

impl AppState {
    /// Create a new application state
    pub fn new(config: Config) -> Self {
        Self {
            config,
            providers: RwLock::new(HashMap::new()),
            wallets: RwLock::new(HashMap::new()),
            price_cache: Mutex::new(HashMap::new()),
        }
    }
}

/// Error type for the arbitrage bot
#[derive(Error, Debug)]
pub enum ArbitrageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("JSON5 error: {0}")]
    Json5(#[from] json5::Error),

    #[error("Ethereum provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Ethereum contract error: {0}")]
    Contract(#[from] ContractError<Provider<Ws>>),

    #[error("Wallet error: {0}")]
    Wallet(#[from] WalletError),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Strategy error: {0}")]
    Strategy(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Simulation error: {0}")]
    Simulation(String),

    #[error("Flashbots error: {0}")]
    Flashbots(String),

    #[error("Tenderly error: {0}")]
    Tenderly(String),

    #[error("Timeout error: {0}")]
    Timeout(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Result type for the arbitrage bot
pub type ArbitrageResult<T> = Result<T, ArbitrageError>;

impl Config {
    /// Load configuration from a file
    pub async fn from_file<P: AsRef<Path>>(path: P) -> ArbitrageResult<Self> {
        let content = tokio::fs::read_to_string(path).await?;
        let config: Config = json5::from_str(&content)?;
        Ok(config)
    }

    /// Validate the configuration
    pub fn validate(&self) -> ArbitrageResult<()> {
        // Check if at least one network is configured
        if self.networks.is_empty() {
            return Err(ArbitrageError::Config("No networks configured".to_string()));
        }

        // Check if RPC URLs are provided for all networks
        for network in &self.networks {
            if !self.rpc_urls.contains_key(network) {
                return Err(ArbitrageError::Config(format!(
                    "No RPC URL provided for network {}",
                    network.name()
                )));
            }
        }

        // Check if wallet configuration is valid
        if self.wallet.private_key.is_empty() {
            return Err(ArbitrageError::Config("No private key provided".to_string()));
        }

        if self.wallet.address.is_empty() {
            return Err(ArbitrageError::Config("No wallet address provided".to_string()));
        }

        Ok(())
    }
}
