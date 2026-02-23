use crate::{Router, RouterClient, RouterError};
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::Address as _,
    testutils::Ledger as _,
    token::{StellarAssetClient, TokenClient},
    Address, Env,
};

// ── Mock Contracts ────────────────────────────────────────────────────────────

/// Minimal mock factory that stores and returns a pair address.
#[contract]
pub struct MockFactory;

#[contracttype]
#[derive(Clone)]
pub enum MFKey {
    Pair(Address, Address),
}

#[contractimpl]
impl MockFactory {
    pub fn set_pair(env: Env, token_a: Address, token_b: Address, pair: Address) {
        env.storage().instance().set(&MFKey::Pair(token_a, token_b), &pair);
    }

    pub fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address> {
        env.storage().instance().get(&MFKey::Pair(token_a, token_b))
    }
}

/// Minimal mock pair that returns pre-configured burn amounts and LP token.
#[contract]
pub struct MockPair;

#[contracttype]
#[derive(Clone)]
pub enum MPKey {
    LpToken,
    AmountA,
    AmountB,
}

#[contractimpl]
impl MockPair {
    pub fn set_lp_token(env: Env, lp_token: Address) {
        env.storage().instance().set(&MPKey::LpToken, &lp_token);
    }

    pub fn set_burn_amounts(env: Env, amount_a: i128, amount_b: i128) {
        env.storage().instance().set(&MPKey::AmountA, &amount_a);
        env.storage().instance().set(&MPKey::AmountB, &amount_b);
    }

    pub fn lp_token(env: Env) -> Address {
        env.storage().instance().get(&MPKey::LpToken).unwrap()
    }

    pub fn burn(env: Env, _to: Address) -> (i128, i128) {
        let a: i128 = env.storage().instance().get(&MPKey::AmountA).unwrap();
        let b: i128 = env.storage().instance().get(&MPKey::AmountB).unwrap();
        (a, b)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Sets up a full mock environment with Router, Factory, Pair, and LP token.
///
/// Returns (env, router_client, token_a, token_b, to, deadline,
///          mock_pair_client, lp_token_addr, pair_addr).
#[allow(clippy::type_complexity)]
fn setup_full_env() -> (
    Env,
    RouterClient<'static>,
    Address,
    Address,
    Address,
    u64,
    MockPairClient<'static>,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    // Register contracts
    let router_addr = env.register_contract(None, Router);
    let router_client = RouterClient::new(&env, &router_addr);

    let factory_addr = env.register_contract(None, MockFactory);
    let mock_factory_client = MockFactoryClient::new(&env, &factory_addr);

    let pair_addr = env.register_contract(None, MockPair);
    let mock_pair_client = MockPairClient::new(&env, &pair_addr);

    // Create LP token via Stellar Asset Contract
    let lp_admin = Address::generate(&env);
    let lp_token_addr = env.register_stellar_asset_contract_v2(lp_admin.clone()).address();
    let lp_sac_client = StellarAssetClient::new(&env, &lp_token_addr);

    // Generate token addresses (must be different)
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    // Wire up: Router -> Factory -> Pair -> LP Token
    router_client.initialize(&factory_addr);
    mock_factory_client.set_pair(&token_a, &token_b, &pair_addr);
    mock_pair_client.set_lp_token(&lp_token_addr);
    mock_pair_client.set_burn_amounts(&500, &1000);

    // Mint LP tokens to the caller
    lp_sac_client.mint(&to, &2000);

    let deadline = env.ledger().timestamp() + 1000;

    (env, router_client, token_a, token_b, to, deadline, mock_pair_client, lp_token_addr, pair_addr)
}

// ── Placeholder tests (other functions still todo) ────────────────────────────

#[test]
fn test_placeholder_swap_exact_in() {
    let _env = Env::default();
}

#[test]
fn test_placeholder_swap_tokens_for_exact_tokens() {
    let _env = Env::default();
}

#[test]
fn test_placeholder_add_liquidity() {
    let _env = Env::default();
}

// ── remove_liquidity tests ────────────────────────────────────────────────────

#[test]
fn test_remove_liquidity_expired_deadline() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    // Move ledger time forward so we can set a past deadline
    env.ledger().set_timestamp(2000);
    let past_deadline = env.ledger().timestamp() - 1000;

    let result = router.try_remove_liquidity(
        &token_a,
        &token_b,
        &100i128,
        &50i128,
        &50i128,
        &to,
        &past_deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::Expired)));
}

#[test]
fn test_remove_liquidity_zero_amount() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    let deadline = env.ledger().timestamp() + 1000;

    let result = router.try_remove_liquidity(
        &token_a, &token_b, &0i128, // zero liquidity
        &50i128, &50i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::ZeroAmount)));
}

#[test]
fn test_remove_liquidity_identical_tokens() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    let deadline = env.ledger().timestamp() + 1000;

    // Pass the same address for both tokens
    let result = router.try_remove_liquidity(
        &token_a, &token_a, // identical
        &100i128, &50i128, &50i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::IdenticalTokens)));
}

#[test]
fn test_remove_liquidity_pair_not_found() {
    let env = Env::default();
    env.mock_all_auths();

    let factory_addr = env.register_contract(None, MockFactory);
    let router_addr = env.register_contract(None, Router);
    let router = RouterClient::new(&env, &router_addr);

    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_addr);

    // Factory has no pair registered
    let deadline = env.ledger().timestamp() + 1000;

    let result =
        router.try_remove_liquidity(&token_a, &token_b, &100i128, &50i128, &50i128, &to, &deadline);

    assert_eq!(result, Err(Ok(RouterError::PairNotFound)));
}

#[test]
fn test_remove_liquidity_happy_path() {
    let (
        _env,
        router_client,
        token_a,
        token_b,
        to,
        deadline,
        _mock_pair_client,
        _lp_token_addr,
        _pair_addr,
    ) = setup_full_env();

    // Mock pair returns (500, 1000) on burn; minimums are below those values
    let result = router_client.remove_liquidity(
        &token_a, &token_b, &100i128, // liquidity to burn
        &400i128, // amount_a_min (below 500 returned)
        &900i128, // amount_b_min (below 1000 returned)
        &to, &deadline,
    );

    assert_eq!(result, (500, 1000));
}

#[test]
fn test_remove_liquidity_exact_minimums() {
    let (
        _env,
        router_client,
        token_a,
        token_b,
        to,
        deadline,
        _mock_pair_client,
        _lp_token_addr,
        _pair_addr,
    ) = setup_full_env();

    // amount_a_min == amount_a and amount_b_min == amount_b (edge case)
    let result = router_client.remove_liquidity(
        &token_a, &token_b, &100i128, &500i128,  // exactly equal to returned amount_a
        &1000i128, // exactly equal to returned amount_b
        &to, &deadline,
    );

    assert_eq!(result, (500, 1000));
}

#[test]
fn test_remove_liquidity_insufficient_output_amount_a() {
    let (
        _env,
        router_client,
        token_a,
        token_b,
        to,
        deadline,
        _mock_pair_client,
        _lp_token_addr,
        _pair_addr,
    ) = setup_full_env();

    // amount_a_min > returned amount_a (500) → slippage revert
    let result = router_client.try_remove_liquidity(
        &token_a, &token_b, &100i128, &600i128, // above 500 returned
        &900i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::InsufficientOutputAmount)));
}

#[test]
fn test_remove_liquidity_insufficient_output_amount_b() {
    let (
        _env,
        router_client,
        token_a,
        token_b,
        to,
        deadline,
        _mock_pair_client,
        _lp_token_addr,
        _pair_addr,
    ) = setup_full_env();

    // amount_b_min > returned amount_b (1000) → slippage revert
    let result = router_client.try_remove_liquidity(
        &token_a, &token_b, &100i128, &400i128, &1100i128, // above 1000 returned
        &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::InsufficientOutputAmount)));
}

#[test]
fn test_remove_liquidity_lp_tokens_transferred() {
    let (
        _env,
        router_client,
        token_a,
        token_b,
        to,
        deadline,
        _mock_pair_client,
        lp_token_addr,
        pair_addr,
    ) = setup_full_env();

    let lp_token = TokenClient::new(&_env, &lp_token_addr);
    let balance_before = lp_token.balance(&to);

    let liquidity: i128 = 200;
    router_client
        .remove_liquidity(&token_a, &token_b, &liquidity, &400i128, &900i128, &to, &deadline);

    // LP tokens should have been transferred from 'to' to pair
    let balance_after = lp_token.balance(&to);
    assert_eq!(balance_after, balance_before - liquidity);

    // Pair should have received the LP tokens
    let pair_balance = lp_token.balance(&pair_addr);
    assert_eq!(pair_balance, liquidity);
}
