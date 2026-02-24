#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std;

mod errors;
mod helpers;
mod storage;

#[cfg(test)]
mod test;

use errors::RouterError;
use helpers::{get_pair_address, quote, sort_tokens, FactoryClient, PairClient};
use soroban_sdk::{contract, contractimpl, token::TokenClient, Address, Env, Vec};
use storage::{get_factory, set_factory};

#[contract]
pub struct Router;

#[contractimpl]
impl Router {
    pub fn initialize(env: Env, factory: Address) {
        set_factory(&env, &factory);
    }
    pub fn swap_exact_tokens_for_tokens(
        _env: Env,
        _amount_in: i128,
        _amount_out_min: i128,
        _path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, RouterError> {
        todo!()
    }

    /// Swaps tokens to receive an exact amount of output tokens (not yet implemented).
    ///
    /// # Arguments
    /// * `amount_out` - The exact amount of output tokens desired
    /// * `amount_in_max` - The maximum amount of input tokens to spend
    /// * `path` - Vector of token addresses representing the swap route
    /// * `to` - The recipient address for output tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn swap_tokens_for_exact_tokens(
        _env: Env,
        _amount_out: i128,
        _amount_in_max: i128,
        _path: Vec<Address>,
        _to: Address,
        _deadline: u64,
    ) -> Result<Vec<i128>, RouterError> {
        todo!()
    }

    /// Adds liquidity to a token pair (not yet implemented).
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `amount_a_desired` - Desired amount of token_a to add
    /// * `amount_b_desired` - Desired amount of token_b to add
    /// * `amount_a_min` - Minimum amount of token_a to add
    /// * `amount_b_min` - Minimum amount of token_b to add
    /// * `to` - Recipient of LP tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn add_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
    ) -> Result<(i128, i128, i128), RouterError> {
        if deadline < env.ledger().timestamp() {
            return Err(RouterError::Expired);
        }

        let (amount_a, amount_b) = Self::_add_liquidity(
            &env,
            &token_a,
            &token_b,
            amount_a_desired,
            amount_b_desired,
            amount_a_min,
            amount_b_min,
        )?;

        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;
        let pair_address = get_pair_address(&env, &factory, &token_a, &token_b)?;

        // Transfer tokens to pair
        to.require_auth();

        TokenClient::new(&env, &token_a).transfer(&to, &pair_address, &amount_a);
        TokenClient::new(&env, &token_b).transfer(&to, &pair_address, &amount_b);

        let liquidity = PairClient::new(&env, &pair_address).mint(&to);

        Ok((amount_a, amount_b, liquidity))
    }

    fn _add_liquidity(
        env: &Env,
        token_a: &Address,
        token_b: &Address,
        amount_a_desired: i128,
        amount_b_desired: i128,
        amount_a_min: i128,
        amount_b_min: i128,
    ) -> Result<(i128, i128), RouterError> {
        let factory = get_factory(env).ok_or(RouterError::PairNotFound)?;
        let mut pair_address = get_pair_address(env, &factory, token_a, token_b).ok();

        if pair_address.is_none() {
            pair_address = Some(FactoryClient::new(env, &factory).create_pair(token_a, token_b));
        }

        let pair_client = PairClient::new(env, &pair_address.unwrap());
        let (reserve_a, reserve_b, _) = pair_client.get_reserves();

        if reserve_a == 0 && reserve_b == 0 {
            Ok((amount_a_desired, amount_b_desired))
        } else {
            let (token_0, _) = sort_tokens(token_a, token_b)?;
            let (reserve_0, reserve_1) =
                if token_a == &token_0 { (reserve_a, reserve_b) } else { (reserve_b, reserve_a) };

            let amount_b_optimal = quote(amount_a_desired, reserve_0, reserve_1)?;
            if amount_b_optimal <= amount_b_desired {
                if amount_b_optimal < amount_b_min {
                    return Err(RouterError::SlippageExceeded);
                }
                Ok((amount_a_desired, amount_b_optimal))
            } else {
                let amount_a_optimal = quote(amount_b_desired, reserve_1, reserve_0)?;
                if amount_a_optimal > amount_a_desired {
                    // This shouldn't happen if quote is correct
                    return Err(RouterError::InternalError);
                }
                if amount_a_optimal < amount_a_min {
                    return Err(RouterError::SlippageExceeded);
                }
                Ok((amount_a_optimal, amount_b_desired))
            }
        }
    }

    /// Removes liquidity from a token pair (not yet implemented).
    ///
    /// # Arguments
    /// * `token_a` - First token address
    /// * `token_b` - Second token address
    /// * `liquidity` - Amount of LP tokens to burn
    /// * `amount_a_min` - Minimum amount of token_a to receive
    /// * `amount_b_min` - Minimum amount of token_b to receive
    /// * `to` - Recipient of underlying tokens
    /// * `deadline` - Unix timestamp after which the transaction will revert
    pub fn remove_liquidity(
        env: Env,
        token_a: Address,
        token_b: Address,
        liquidity: i128,
        amount_a_min: i128,
        amount_b_min: i128,
        to: Address,
        deadline: u64,
    ) -> Result<(i128, i128), RouterError> {
        // Check deadline
        if deadline < env.ledger().timestamp() {
            return Err(RouterError::Expired);
        }

        // Check for non-zero liquidity
        if liquidity <= 0 {
            return Err(RouterError::ZeroAmount);
        }

        // Check for identical tokens
        if token_a == token_b {
            return Err(RouterError::IdenticalTokens);
        }

        // Get factory address
        let factory = get_factory(&env).ok_or(RouterError::PairNotFound)?;

        // Get pair address
        let pair_address = get_pair_address(&env, &factory, &token_a, &token_b)?;

        // Get pair contract client
        let pair_client = PairClient::new(&env, &pair_address);

        // Get LP token address from pair
        let lp_token_address = pair_client.lp_token();

        // The user must provide authorization for the Router to transfer LP tokens
        to.require_auth();

        // Transfer LP tokens from 'to' to pair
        let lp_token_client = TokenClient::new(&env, &lp_token_address);
        lp_token_client.transfer(&to, &pair_address, &liquidity);

        // Call Pair::burn(to) - this will burn LP tokens from the pair and transfer underlying tokens
        let (amount_a, amount_b) = pair_client.burn(&to);

        // Enforce minimum output amounts
        if amount_a < amount_a_min || amount_b < amount_b_min {
            return Err(RouterError::InsufficientOutputAmount);
        }

        Ok((amount_a, amount_b))
    }
}
