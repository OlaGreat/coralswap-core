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
    ReserveA,
    ReserveB,
    MintReturn,
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

    pub fn set_reserves(env: Env, reserve_a: i128, reserve_b: i128) {
        env.storage().instance().set(&MPKey::ReserveA, &reserve_a);
        env.storage().instance().set(&MPKey::ReserveB, &reserve_b);
    }

    pub fn set_mint_return(env: Env, liquidity: i128) {
        env.storage().instance().set(&MPKey::MintReturn, &liquidity);
    }

    pub fn get_reserves(env: Env) -> (i128, i128, u64) {
        let ra: i128 = env.storage().instance().get(&MPKey::ReserveA).unwrap_or(0);
        let rb: i128 = env.storage().instance().get(&MPKey::ReserveB).unwrap_or(0);
        (ra, rb, 0u64)
    }

    pub fn mint(env: Env, _to: Address) -> i128 {
        env.storage().instance().get(&MPKey::MintReturn).unwrap_or(0)
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

// ── add_liquidity test setup ──────────────────────────────────────────────────

/// Sets up a full mock environment for add_liquidity tests with real token contracts.
///
/// Returns (env, router_client, token_a, token_b, to, deadline,
///          mock_pair_client, pair_addr).
#[allow(clippy::type_complexity)]
fn setup_add_liquidity_env(
) -> (Env, RouterClient<'static>, Address, Address, Address, u64, MockPairClient<'static>, Address)
{
    let env = Env::default();
    env.mock_all_auths();

    // Register contracts
    let router_addr = env.register_contract(None, Router);
    let router_client = RouterClient::new(&env, &router_addr);

    let factory_addr = env.register_contract(None, MockFactory);
    let mock_factory_client = MockFactoryClient::new(&env, &factory_addr);

    let pair_addr = env.register_contract(None, MockPair);
    let mock_pair_client = MockPairClient::new(&env, &pair_addr);

    // Create real token contracts for token_a and token_b
    let admin_a = Address::generate(&env);
    let token_a = env.register_stellar_asset_contract_v2(admin_a).address();
    let sac_a = StellarAssetClient::new(&env, &token_a);

    let admin_b = Address::generate(&env);
    let token_b = env.register_stellar_asset_contract_v2(admin_b).address();
    let sac_b = StellarAssetClient::new(&env, &token_b);

    let to = Address::generate(&env);

    // Wire up: Router -> Factory -> Pair
    router_client.initialize(&factory_addr);
    mock_factory_client.set_pair(&token_a, &token_b, &pair_addr);
    mock_pair_client.set_reserves(&0, &0);
    mock_pair_client.set_mint_return(&1000);

    // Mint tokens to the user
    sac_a.mint(&to, &100_000);
    sac_b.mint(&to, &100_000);

    let deadline = env.ledger().timestamp() + 1000;

    (env, router_client, token_a, token_b, to, deadline, mock_pair_client, pair_addr)
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

// ── add_liquidity tests ───────────────────────────────────────────────────────

#[test]
fn test_add_liquidity_expired_deadline() {
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

    let result = router.try_add_liquidity(
        &token_a,
        &token_b,
        &1000i128,
        &1000i128,
        &500i128,
        &500i128,
        &to,
        &past_deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::Expired)));
}

#[test]
fn test_add_liquidity_zero_amount_a() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    let deadline = env.ledger().timestamp() + 1000;

    let result = router.try_add_liquidity(
        &token_a, &token_b, &0i128, // zero amount_a_desired
        &1000i128, &0i128, &500i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::ZeroAmount)));
}

#[test]
fn test_add_liquidity_zero_amount_b() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    let deadline = env.ledger().timestamp() + 1000;

    let result = router.try_add_liquidity(
        &token_a, &token_b, &1000i128, &0i128, // zero amount_b_desired
        &500i128, &0i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::ZeroAmount)));
}

#[test]
fn test_add_liquidity_identical_tokens() {
    let env = Env::default();
    let router = RouterClient::new(&env, &env.register_contract(None, Router));

    let factory_address = Address::generate(&env);
    let token_a = Address::generate(&env);
    let to = Address::generate(&env);

    router.initialize(&factory_address);

    let deadline = env.ledger().timestamp() + 1000;

    // Pass the same address for both tokens
    let result = router.try_add_liquidity(
        &token_a, &token_a, // identical
        &1000i128, &1000i128, &500i128, &500i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::IdenticalTokens)));
}

#[test]
fn test_add_liquidity_pair_not_found() {
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

    let result = router.try_add_liquidity(
        &token_a, &token_b, &1000i128, &1000i128, &500i128, &500i128, &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::PairNotFound)));
}

#[test]
fn test_add_liquidity_first_deposit() {
    let (_env, router_client, token_a, token_b, to, deadline, _mock_pair_client, _pair_addr) =
        setup_add_liquidity_env();

    // Reserves are zero (first deposit) -- desired amounts are used as-is
    let result = router_client.add_liquidity(
        &token_a, &token_b, &5000i128, &10000i128, &5000i128, &10000i128, &to, &deadline,
    );

    // Amounts should equal desired, liquidity from mock is 1000
    assert_eq!(result, (5000, 10000, 1000));
}

#[test]
fn test_add_liquidity_proportional_deposit_b_optimal() {
    let (_env, router_client, token_a, token_b, to, deadline, mock_pair_client, _pair_addr) =
        setup_add_liquidity_env();

    // Set existing reserves at 1:2 ratio
    mock_pair_client.set_reserves(&1000, &2000);
    mock_pair_client.set_mint_return(&500);

    // amount_b_optimal = 1000 * 2000 / 1000 = 2000 <= 3000 (desired_b)
    // So we deposit (1000, 2000)
    let result = router_client.add_liquidity(
        &token_a, &token_b, &1000i128, // amount_a_desired
        &3000i128, // amount_b_desired
        &500i128,  // amount_a_min
        &1000i128, // amount_b_min
        &to, &deadline,
    );

    assert_eq!(result, (1000, 2000, 500));
}

#[test]
fn test_add_liquidity_proportional_deposit_a_optimal() {
    let (_env, router_client, token_a, token_b, to, deadline, mock_pair_client, _pair_addr) =
        setup_add_liquidity_env();

    // Set existing reserves at 1:2 ratio
    mock_pair_client.set_reserves(&1000, &2000);
    mock_pair_client.set_mint_return(&250);

    // amount_b_optimal = 2000 * 2000 / 1000 = 4000 > 2000 (desired_b)
    // So compute amount_a_optimal = 2000 * 1000 / 2000 = 1000
    // Deposit (1000, 2000)
    let result = router_client.add_liquidity(
        &token_a, &token_b, &2000i128, // amount_a_desired
        &2000i128, // amount_b_desired
        &500i128,  // amount_a_min
        &1000i128, // amount_b_min
        &to, &deadline,
    );

    assert_eq!(result, (1000, 2000, 250));
}

#[test]
fn test_add_liquidity_slippage_revert_b() {
    let (_env, router_client, token_a, token_b, to, deadline, mock_pair_client, _pair_addr) =
        setup_add_liquidity_env();

    // Set existing reserves at 1:2 ratio
    mock_pair_client.set_reserves(&1000, &2000);

    // amount_b_optimal = 500 * 2000 / 1000 = 1000 <= 1500 (desired_b)
    // But 1000 < 1200 (amount_b_min) → slippage revert
    let result = router_client.try_add_liquidity(
        &token_a, &token_b, &500i128,  // amount_a_desired
        &1500i128, // amount_b_desired
        &400i128,  // amount_a_min
        &1200i128, // amount_b_min (above optimal)
        &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::SlippageExceeded)));
}

#[test]
fn test_add_liquidity_slippage_revert_a() {
    let (_env, router_client, token_a, token_b, to, deadline, mock_pair_client, _pair_addr) =
        setup_add_liquidity_env();

    // Set existing reserves at 1:2 ratio
    mock_pair_client.set_reserves(&1000, &2000);

    // amount_b_optimal = 3000 * 2000 / 1000 = 6000 > 2000 (desired_b)
    // so compute amount_a_optimal = 2000 * 1000 / 2000 = 1000
    // 1000 < 1500 (amount_a_min) → slippage revert
    let result = router_client.try_add_liquidity(
        &token_a, &token_b, &3000i128, // amount_a_desired
        &2000i128, // amount_b_desired
        &1500i128, // amount_a_min (above optimal)
        &1000i128, // amount_b_min
        &to, &deadline,
    );

    assert_eq!(result, Err(Ok(RouterError::SlippageExceeded)));
}

#[test]
fn test_add_liquidity_tokens_transferred_to_pair() {
    let (_env, router_client, token_a, token_b, to, deadline, _mock_pair_client, pair_addr) =
        setup_add_liquidity_env();

    let token_a_client = TokenClient::new(&_env, &token_a);
    let token_b_client = TokenClient::new(&_env, &token_b);
    let balance_a_before = token_a_client.balance(&to);
    let balance_b_before = token_b_client.balance(&to);

    // First deposit: amounts used as-is
    router_client.add_liquidity(
        &token_a, &token_b, &3000i128, &5000i128, &3000i128, &5000i128, &to, &deadline,
    );

    // User balance should decrease by deposited amounts
    assert_eq!(token_a_client.balance(&to), balance_a_before - 3000);
    assert_eq!(token_b_client.balance(&to), balance_b_before - 5000);

    // Pair should have received the tokens
    assert_eq!(token_a_client.balance(&pair_addr), 3000);
    assert_eq!(token_b_client.balance(&pair_addr), 5000);
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
