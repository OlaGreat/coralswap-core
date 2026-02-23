#![allow(dead_code)]

use crate::errors::RouterError;
use soroban_sdk::{contractclient, Address, Env};

#[contractclient(name = "FactoryClient")]
#[allow(dead_code)]
pub trait FactoryInterface {
    fn get_pair(env: Env, token_a: Address, token_b: Address) -> Option<Address>;
}

#[contractclient(name = "PairClient")]
#[allow(dead_code)]
pub trait PairInterface {
    fn burn(env: Env, to: Address) -> (i128, i128);
    fn lp_token(env: Env) -> Address;
    fn swap(env: Env, amount_a_out: i128, amount_b_out: i128, to: Address);
    fn get_reserves(env: Env) -> (i128, i128, u64);
    fn get_current_fee_bps(env: Env) -> u32;
}

#[contractclient(name = "TokenClient")]
#[allow(dead_code)]
pub trait TokenInterface {
    fn transfer(env: Env, from: Address, to: Address, amount: i128);
    fn balance(env: Env, id: Address) -> i128;
}

/// Computes output amount for an exact input swap using constant-product formula.
///
/// Formula: amount_out = (amount_in * (10000 - fee_bps) * reserve_out) /
///                       (reserve_in * 10000 + amount_in * (10000 - fee_bps))
///
/// # Arguments
/// * `amount_in` - The input token amount
/// * `reserve_in` - The reserve of the input token in the pair
/// * `reserve_out` - The reserve of the output token in the pair
/// * `fee_bps` - The fee in basis points (e.g., 30 = 0.3%)
#[allow(dead_code)]
pub fn get_amount_out(
    _env: &Env,
    amount_in: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, RouterError> {
    todo!()
}

/// Computes input amount required for an exact output swap.
///
/// Formula: amount_in = (reserve_in * amount_out * 10000) /
///                      ((reserve_out - amount_out) * (10000 - fee_bps)) + 1
#[allow(dead_code)]
pub fn get_amount_in(
    _env: &Env,
    amount_out: i128,
    reserve_in: i128,
    reserve_out: i128,
    fee_bps: u32,
) -> Result<i128, RouterError> {
    todo!()
}

/// Sorts token addresses into canonical order (lexicographically).
///
/// Returns tokens in the order (token_a, token_b) where token_a < token_b.
/// This matches the ordering used by the Factory when creating pairs.
#[allow(dead_code)]
pub fn sort_tokens(
    _token_a: &Address,
    _token_b: &Address,
) -> Result<(Address, Address), RouterError> {
    todo!()
}

/// Get the pair address from the factory contract
pub fn get_pair_address(
    env: &Env,
    factory: &Address,
    token_a: &Address,
    token_b: &Address,
) -> Result<Address, RouterError> {
    let factory_client = FactoryClient::new(env, factory);
    factory_client.get_pair(token_a, token_b).ok_or(RouterError::PairNotFound)
}
