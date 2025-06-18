// benches/arbitrage_benchmarks.rs
#![feature(test)]
extern crate test;

use defi_arbitrage_bot::common::{
    AppState, ArbitrageOpportunity, Config, Network, SwapStep, Token,
};
use defi_arbitrage_bot::executors::{Executor, StandardExecutor};
use defi_arbitrage_bot::simulators::{LocalSimulator, Simulator};
use defi_arbitrage_bot::strategies::{
    CrossExchangeArbitrageStrategy, Strategy, TriangularArbitrageStrategy,
};
use ethers::prelude::*;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use test::Bencher;
use uuid::Uuid;

// Mock data for benchmarks
fn create_mock_app_state() -> Arc<AppState> {
    let config = Config {
        general: Default::default(),
        networks: vec![Network::Mainnet],
        rpc_urls: Default::default(),
        wallet: Default::default(),
        flashbots: Default::default(),
        tenderly: None,
        strategies: Default::default(),
        dexes: Default::default(),
        tokens: Default::default(),
    };

    Arc::new(AppState::new(config))
}

fn create_mock_opportunity() -> ArbitrageOpportunity {
    let token_a = Token {
        address: Address::random(),
        symbol: "WETH".to_string(),
        name: "Wrapped Ether".to_string(),
        decimals: 18,
        network: Network::Mainnet,
    };

    let token_b = Token {
        address: Address::random(),
        symbol: "USDC".to_string(),
        name: "USD Coin".to_string(),
        decimals: 6,
        network: Network::Mainnet,
    };

    let token_c = Token {
        address: Address::random(),
        symbol: "DAI".to_string(),
        name: "Dai Stablecoin".to_string(),
        decimals: 18,
        network: Network::Mainnet,
    };

    let route = vec![
        SwapStep {
            dex: "uniswap_v2".to_string(),
            pool_address: Address::random(),
            token_in: token_a.clone(),
            token_out: token_b.clone(),
            amount_in: U256::from(1000000000000000000u64), // 1 ETH
            expected_amount_out: U256::from(1800000000u64), // 1800 USDC
        },
        SwapStep {
            dex: "sushiswap".to_string(),
            pool_address: Address::random(),
            token_in: token_b.clone(),
            token_out: token_c.clone(),
            amount_in: U256::from(1800000000u64), // 1800 USDC
            expected_amount_out: U256::from(1810000000000000000000u128), // 1810 DAI
        },
        SwapStep {
            dex: "curve".to_string(),
            pool_address: Address::random(),
            token_in: token_c.clone(),
            token_out: token_a.clone(),
            amount_in: U256::from(1810000000000000000000u128), // 1810 DAI
            expected_amount_out: U256::from(1020000000000000000u64), // 1.02 ETH
        },
    ];

    ArbitrageOpportunity {
        id: Uuid::new_v4().to_string(),
        network: Network::Mainnet,
        route,
        input_token: token_a.clone(),
        input_amount: U256::from(1000000000000000000u64), // 1 ETH
        expected_output: U256::from(1020000000000000000u64), // 1.02 ETH
        expected_profit: U256::from(20000000000000000u64), // 0.02 ETH
        expected_profit_usd: 40.0,                        // $40
        gas_cost_usd: 10.0,                               // $10
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        status: "pending".to_string(),
    }
}

#[bench]
fn bench_triangular_strategy_find_opportunities(b: &mut Bencher) {
    let app_state = create_mock_app_state();
    let strategy = TriangularArbitrageStrategy::new(
        app_state,
        Network::Mainnet,
        10.0, // $10 minimum profit
    );

    b.iter(|| {
        // This is just a mock benchmark since we can't actually run the strategy
        // without real blockchain data
        test::black_box(&strategy);
    });
}

#[bench]
fn bench_cross_exchange_strategy_find_opportunities(b: &mut Bencher) {
    let app_state = create_mock_app_state();
    let strategy = CrossExchangeArbitrageStrategy::new(
        app_state,
        Network::Mainnet,
        10.0, // $10 minimum profit
    );

    b.iter(|| {
        // This is just a mock benchmark since we can't actually run the strategy
        // without real blockchain data
        test::black_box(&strategy);
    });
}

#[bench]
fn bench_local_simulator_simulate(b: &mut Bencher) {
    let app_state = create_mock_app_state();
    let simulator = LocalSimulator::new(app_state);
    let opportunity = create_mock_opportunity();

    b.iter(|| {
        // This is just a mock benchmark since we can't actually run the simulator
        // without real blockchain data
        test::black_box((&simulator, &opportunity));
    });
}

#[bench]
fn bench_standard_executor_execute(b: &mut Bencher) {
    let app_state = create_mock_app_state();
    let executor = StandardExecutor::new(app_state);
    let opportunity = create_mock_opportunity();

    b.iter(|| {
        // This is just a mock benchmark since we can't actually run the executor
        // without real blockchain data
        test::black_box((&executor, &opportunity));
    });
}

#[bench]
fn bench_calculate_arbitrage_profit(b: &mut Bencher) {
    let opportunity = create_mock_opportunity();

    b.iter(|| {
        // Calculate net profit
        let profit = opportunity.expected_profit;
        let profit_usd = opportunity.expected_profit_usd;
        let gas_cost_usd = opportunity.gas_cost_usd;
        let net_profit_usd = profit_usd - gas_cost_usd;

        test::black_box((profit, net_profit_usd));
    });
}

#[bench]
fn bench_validate_arbitrage_route(b: &mut Bencher) {
    let opportunity = create_mock_opportunity();

    b.iter(|| {
        // Validate that the route is valid (last token out = first token in)
        let route = &opportunity.route;
        let valid = !route.is_empty()
            && route.last().unwrap().token_out.address == opportunity.input_token.address;

        test::black_box(valid);
    });
}

#[bench]
fn bench_sort_opportunities_by_profit(b: &mut Bencher) {
    let mut opportunities = vec![
        create_mock_opportunity(),
        create_mock_opportunity(),
        create_mock_opportunity(),
        create_mock_opportunity(),
        create_mock_opportunity(),
    ];

    // Modify profits to ensure they're different
    opportunities[0].expected_profit_usd = 50.0;
    opportunities[1].expected_profit_usd = 30.0;
    opportunities[2].expected_profit_usd = 70.0;
    opportunities[3].expected_profit_usd = 20.0;
    opportunities[4].expected_profit_usd = 60.0;

    b.iter(|| {
        let mut ops = opportunities.clone();
        ops.sort_by(|a, b| {
            b.expected_profit_usd
                .partial_cmp(&a.expected_profit_usd)
                .unwrap()
        });
        test::black_box(&ops);
    });
}
