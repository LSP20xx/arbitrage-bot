# DeFi Arbitrage Bot Platform API Documentation

This document provides detailed API documentation for the DeFi Arbitrage Bot Platform.

## Core Components

### AppState

The `AppState` struct maintains the shared state of the application.

```rust
pub struct AppState {
    pub config: Config,
    pub providers: RwLock<HashMap<Network, Arc<Provider<Ws>>>>,
    pub wallets: RwLock<HashMap<Network, Arc<SignerMiddleware<Arc<Provider<Ws>>, LocalWallet>>>>,
    pub price_cache: Mutex<HashMap<String, f64>>,
}
```

#### Methods

- `new(config: Config) -> Self`: Create a new application state.
- `get_token_by_address(network: Network, address: Address) -> Option<Token>`: Get token details by address.
- `get_equivalent_token(source_network: Network, target_network: Network, token: Address) -> Option<Address>`: Get the equivalent token on another network.

### Network

The `Network` enum represents supported blockchain networks.

```rust
pub enum Network {
    Mainnet,
    Arbitrum,
    Optimism,
    Polygon,
    Base,
}
```

#### Methods

- `name(&self) -> &'static str`: Get the name of the network.
- `chain_id(&self) -> u64`: Get the chain ID of the network.
- `from_str(s: &str) -> Option<Self>`: Parse a network from a string.

### ArbitrageOpportunity

The `ArbitrageOpportunity` struct represents an arbitrage opportunity.

```rust
pub struct ArbitrageOpportunity {
    pub id: String,
    pub network: Network,
    pub route: Vec<SwapStep>,
    pub input_token: Token,
    pub input_amount: U256,
    pub expected_output: U256,
    pub expected_profit: U256,
    pub expected_profit_usd: f64,
    pub gas_cost_usd: f64,
    pub timestamp: u64,
    pub status: String,
}
```

### SwapStep

The `SwapStep` struct represents a step in an arbitrage route.

```rust
pub struct SwapStep {
    pub dex: String,
    pub pool_address: Address,
    pub token_in: Token,
    pub token_out: Token,
    pub amount_in: U256,
    pub expected_amount_out: U256,
}
```

### Token

The `Token` struct represents a token.

```rust
pub struct Token {
    pub address: Address,
    pub symbol: String,
    pub name: String,
    pub decimals: u8,
    pub network: Network,
}
```

### ExecutionResult

The `ExecutionResult` struct represents the result of an arbitrage execution.

```rust
pub struct ExecutionResult {
    pub opportunity: ArbitrageOpportunity,
    pub success: bool,
    pub tx_hash: H256,
    pub block_number: U64,
    pub gas_used: U256,
    pub actual_output: U256,
    pub error: Option<String>,
}
```

## Collector API

### Collector Trait

The `Collector` trait defines the interface for collectors.

```rust
#[async_trait]
pub trait Collector: Send + Sync {
    async fn start(&mut self) -> Result<(), ArbitrageError>;
    async fn stop(&mut self) -> Result<(), ArbitrageError>;
    fn is_running(&self) -> bool;
    fn name(&self) -> &str;
    fn collector_type(&self) -> CollectorType;
}
```

### CollectorType

The `CollectorType` enum represents the types of collectors.

```rust
pub enum CollectorType {
    Mempool,
    BlockEvents,
    PriceFeeds,
    PoolReserves,
    Custom,
}
```

### MempoolCollector

The `MempoolCollector` struct monitors pending transactions in the mempool.

```rust
pub struct MempoolCollector {
    config: MempoolCollectorConfig,
    provider: Option<Provider<Ws>>,
    tx_sender: Option<broadcast::Sender<MempoolTransaction>>,
    running: bool,
    name: String,
}
```

#### Methods

- `new(config: MempoolCollectorConfig) -> Self`: Create a new mempool collector.
- `get_sender(&self) -> Option<broadcast::Sender<MempoolTransaction>>`: Get the transaction sender.

### BlockEventsCollector

The `BlockEventsCollector` struct monitors on-chain events from blocks.

```rust
pub struct BlockEventsCollector {
    config: BlockEventsCollectorConfig,
    provider: Option<Arc<Provider<Http>>>,
    event_sender: Option<broadcast::Sender<Log>>,
    running: bool,
    name: String,
    current_block: u64,
}
```

#### Methods

- `new(config: BlockEventsCollectorConfig) -> Self`: Create a new block events collector.
- `get_sender(&self) -> Option<broadcast::Sender<Log>>`: Get the event sender.

### PriceFeedCollector

The `PriceFeedCollector` struct collects token price data from price feeds.

```rust
pub struct PriceFeedCollector {
    config: PriceFeedCollectorConfig,
    provider: Option<Arc<Provider<Http>>>,
    price_sender: Option<broadcast::Sender<PriceData>>,
    running: bool,
    name: String,
}
```

#### Methods

- `new(config: PriceFeedCollectorConfig) -> Self`: Create a new price feed collector.
- `get_sender(&self) -> Option<broadcast::Sender<PriceData>>`: Get the price sender.

### PoolReservesCollector

The `PoolReservesCollector` struct tracks liquidity pool reserves.

```rust
pub struct PoolReservesCollector {
    config: PoolReservesCollectorConfig,
    provider: Option<Arc<Provider<Http>>>,
    reserves_sender: Option<broadcast::Sender<PoolReserves>>,
    running: bool,
    name: String,
    pool_tokens: HashMap<Address, (Address, Address)>,
}
```

#### Methods

- `new(config: PoolReservesCollectorConfig) -> Self`: Create a new pool reserves collector.
- `get_sender(&self) -> Option<broadcast::Sender<PoolReserves>>`: Get the reserves sender.

## Strategy API

### Strategy Trait

The `Strategy` trait defines the interface for strategies.

```rust
#[async_trait]
pub trait Strategy: Send + Sync {
    async fn find_opportunities(&self) -> ArbitrageResult<Vec<ArbitrageOpportunity>>;
    fn name(&self) -> &str;
}
```

### TriangularArbitrageStrategy

The `TriangularArbitrageStrategy` struct identifies opportunities for triangular arbitrage within a single DEX.

```rust
pub struct TriangularArbitrageStrategy {
    app_state: Arc<AppState>,
    network: Network,
    min_profit_threshold: f64,
}
```

#### Methods

- `new(app_state: Arc<AppState>, network: Network, min_profit_threshold: f64) -> Self`: Create a new triangular arbitrage strategy.

### CrossExchangeArbitrageStrategy

The `CrossExchangeArbitrageStrategy` struct identifies opportunities for arbitrage across different DEXes on the same chain.

```rust
pub struct CrossExchangeArbitrageStrategy {
    app_state: Arc<AppState>,
    network: Network,
    min_profit_threshold: f64,
}
```

#### Methods

- `new(app_state: Arc<AppState>, network: Network, min_profit_threshold: f64) -> Self`: Create a new cross-exchange arbitrage strategy.

### CrossChainArbitrageStrategy

The `CrossChainArbitrageStrategy` struct identifies opportunities for arbitrage across different blockchain networks.

```rust
pub struct CrossChainArbitrageStrategy {
    app_state: Arc<AppState>,
    source_network: Network,
    target_network: Network,
    min_profit_threshold: f64,
}
```

#### Methods

- `new(app_state: Arc<AppState>, source_network: Network, target_network: Network, min_profit_threshold: f64) -> Self`: Create a new cross-chain arbitrage strategy.

### FlashLoanArbitrageStrategy

The `FlashLoanArbitrageStrategy` struct identifies opportunities for arbitrage using flash loans.

```rust
pub struct FlashLoanArbitrageStrategy {
    app_state: Arc<AppState>,
    network: Network,
    min_profit_threshold: f64,
}
```

#### Methods

- `new(app_state: Arc<AppState>, network: Network, min_profit_threshold: f64) -> Self`: Create a new flash loan arbitrage strategy.

## Simulator API

### Simulator Trait

The `Simulator` trait defines the interface for simulators.

```rust
#[async_trait]
pub trait Simulator: Send + Sync {
    async fn simulate(&self, opportunity: ArbitrageOpportunity) -> ArbitrageResult<SimulationResult>;
    fn name(&self) -> &str;
}
```

### SimulationResult

The `SimulationResult` struct represents the result of a simulation.

```rust
pub struct SimulationResult {
    pub opportunity: ArbitrageOpportunity,
    pub success: bool,
    pub actual_output: Option<U256>,
    pub gas_used: Option<U256>,
    pub error: Option<String>,
    pub trace: Option<String>,
}
```

### LocalSimulator

The `LocalSimulator` struct simulates arbitrage execution using local calculations.

```rust
pub struct LocalSimulator {
    app_state: Arc<AppState>,
}
```

#### Methods

- `new(app_state: Arc<AppState>) -> Self`: Create a new local simulator.

### TenderlySimulator

The `TenderlySimulator` struct simulates arbitrage execution using Tenderly's simulation API.

```rust
pub struct TenderlySimulator {
    api_key: String,
    account_id: String,
    project_slug: String,
    app_state: Arc<AppState>,
    http_client: Client,
}
```

#### Methods

- `new(api_key: String, account_id: String, project_slug: String, app_state: Arc<AppState>) -> Self`: Create a new Tenderly simulator.

### ForkedSimulator

The `ForkedSimulator` struct simulates arbitrage execution on a forked blockchain using Anvil.

```rust
pub struct ForkedSimulator {
    app_state: Arc<AppState>,
    fork_url: String,
    fork_block_number: Option<u64>,
}
```

#### Methods

- `new(app_state: Arc<AppState>, fork_url: String, fork_block_number: Option<u64>) -> Self`: Create a new forked simulator.

## Executor API

### Executor Trait

The `Executor` trait defines the interface for executors.

```rust
#[async_trait]
pub trait Executor: Send + Sync {
    async fn execute(&self, opportunity: ArbitrageOpportunity) -> ArbitrageResult<ExecutionResult>;
    fn name(&self) -> &str;
}
```

### StandardExecutor

The `StandardExecutor` struct executes arbitrage using standard transactions.

```rust
pub struct StandardExecutor {
    app_state: Arc<AppState>,
}
```

#### Methods

- `new(app_state: Arc<AppState>) -> Self`: Create a new standard executor.

### FlashbotsExecutor

The `FlashbotsExecutor` struct executes arbitrage using Flashbots bundles to prevent front-running.

```rust
pub struct FlashbotsExecutor {
    app_state: Arc<AppState>,
    config: FlashbotsConfig,
}
```

#### Methods

- `new(app_state: Arc<AppState>, config: FlashbotsConfig) -> Self`: Create a new Flashbots executor.

## Orchestrator API

### Orchestrator

The `Orchestrator` struct coordinates all components of the system.

```rust
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
```

#### Methods

- `new(app_state: Arc<AppState>, config: OrchestratorConfig, collector_manager: CollectorManager, strategy_manager: StrategyManager, simulation_manager: SimulationManager, execution_manager: ExecutionManager) -> Self`: Create a new orchestrator.
- `start(&mut self) -> ArbitrageResult<()>`: Start the orchestrator.
- `stop(&mut self) -> ArbitrageResult<()>`: Stop the orchestrator.

### OrchestratorConfig

The `OrchestratorConfig` struct represents the configuration for the orchestrator.

```rust
pub struct OrchestratorConfig {
    pub max_concurrent_strategies: usize,
    pub max_concurrent_simulations: usize,
    pub max_concurrent_executions: usize,
    pub strategy_timeout: Duration,
    pub simulation_timeout: Duration,
    pub execution_timeout: Duration,
    pub min_profit_threshold_usd: f64,
    pub circuit_breaker_threshold: usize,
    pub circuit_breaker_reset_time: Duration,
}
```

## Smart Contract API

### ArbitrageExecutor.sol

The `ArbitrageExecutor.sol` contract is the main contract for executing arbitrage transactions.

#### Functions

- `executeArbitrage(ArbitrageParams calldata params) external`: Execute an arbitrage opportunity.
- `executeOperation(address[] calldata assets, uint256[] calldata amounts, uint256[] calldata premiums, address initiator, bytes calldata params) external returns (bool)`: Callback function for Aave flash loans.
- `receiveFlashLoan(address[] memory tokens, uint256[] memory amounts, uint256[] memory feeAmounts, bytes memory userData) external`: Callback function for Balancer flash loans.
- `addOperator(address _operator) external`: Add an operator.
- `removeOperator(address _operator) external`: Remove an operator.
- `approveDex(address _dex) external`: Approve a DEX.
- `removeDex(address _dex) external`: Remove a DEX.
- `approveFlashLoanProvider(address _provider) external`: Approve a flash loan provider.
- `removeFlashLoanProvider(address _provider) external`: Remove a flash loan provider.
- `pause() external`: Pause the contract.
- `unpause() external`: Unpause the contract.
- `rescueFunds(address _token, address _to, uint256 _amount) external`: Rescue tokens stuck in the contract.
- `wrapETH(uint256 _amount) external`: Wrap ETH to WETH.
- `unwrapETH(uint256 _amount) external`: Unwrap WETH to ETH.
- `transferOwnership(address _newOwner) external`: Transfer ownership of the contract.

#### Structs

- `SwapStep`: Represents a step in an arbitrage route.
- `ArbitrageParams`: Represents the parameters for an arbitrage execution.

#### Events

- `ArbitrageExecuted(address indexed executor, address indexed flashLoanToken, uint256 flashLoanAmount, uint256 profit)`: Emitted when an arbitrage is executed.
- `OperatorAdded(address indexed operator)`: Emitted when an operator is added.
- `OperatorRemoved(address indexed operator)`: Emitted when an operator is removed.
- `DexApproved(address indexed dex)`: Emitted when a DEX is approved.
- `DexRemoved(address indexed dex)`: Emitted when a DEX is removed.
- `FlashLoanProviderApproved(address indexed provider)`: Emitted when a flash loan provider is approved.
- `FlashLoanProviderRemoved(address indexed provider)`: Emitted when a flash loan provider is removed.
- `Paused(address indexed account)`: Emitted when the contract is paused.
- `Unpaused(address indexed account)`: Emitted when the contract is unpaused.
- `FundsRescued(address indexed token, address indexed to, uint256 amount)`: Emitted when funds are rescued.

## Error Handling

### ArbitrageError

The `ArbitrageError` enum represents errors that can occur in the arbitrage bot.

```rust
pub enum ArbitrageError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Json5(json5::Error),
    Provider(ProviderError),
    Contract(ContractError<Provider<Ws>>),
    Wallet(WalletError),
    Http(reqwest::Error),
    Config(String),
    Network(String),
    Strategy(String),
    Execution(String),
    Simulation(String),
    Flashbots(String),
    Tenderly(String),
    Timeout(String),
    Unknown(String),
}
```

### ArbitrageResult

The `ArbitrageResult` type is a result type for the arbitrage bot.

```rust
pub type ArbitrageResult<T> = Result<T, ArbitrageError>;
```

## Configuration

### Config

The `Config` struct represents the configuration for the arbitrage bot.

```rust
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
```

#### Methods

- `from_file<P: AsRef<Path>>(path: P) -> ArbitrageResult<Self>`: Load configuration from a file.
- `validate(&self) -> ArbitrageResult<()>`: Validate the configuration.

### FlashbotsConfig

The `FlashbotsConfig` struct represents the configuration for Flashbots.

```rust
pub struct FlashbotsConfig {
    pub enabled: bool,
    pub relay_urls: Vec<String>,
    pub min_bid_percentage: f64,
    pub max_bid_percentage: Option<f64>,
    pub bundle_timeout: Option<u64>,
    pub max_blocks_to_search: Option<u64>,
}
```

### TenderlyConfig

The `TenderlyConfig` struct represents the configuration for Tenderly.

```rust
pub struct TenderlyConfig {
    pub enabled: bool,
    pub user: String,
    pub project: String,
    pub access_key: String,
}
```

### StrategyConfig

The `StrategyConfig` struct represents the configuration for a strategy.

```rust
pub struct StrategyConfig {
    pub enabled: bool,
    pub min_profit_usd: f64,
    pub max_path_length: Option<usize>,
    pub interval: Option<u64>,
}
```

### DexConfig

The `DexConfig` struct represents the configuration for a DEX.

```rust
pub struct DexConfig {
    pub enabled: bool,
    pub factory_address: String,
    pub router_address: Option<String>,
}
```

### TokenConfig

The `TokenConfig` struct represents the configuration for a token.

```rust
pub struct TokenConfig {
    pub mainnet: Option<String>,
    pub arbitrum: Option<String>,
    pub optimism: Option<String>,
    pub polygon: Option<String>,
    pub base: Option<String>,
}
```

#### Methods

- `address_for_network(&self, network: Network) -> Option<String>`: Get the address for a specific network.
