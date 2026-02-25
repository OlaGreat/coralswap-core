#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate std; // soroban-sdk testutils require std; pair is no_std so we opt-in explicitly

mod dynamic_fee;
mod errors;
mod events;
mod fee_decay;
mod flash_loan;
mod math;
mod oracle;
mod reentrancy;
mod storage;

#[cfg(test)]
mod test;

use errors::PairError;
use events::PairEvents;
use math::MINIMUM_LIQUIDITY;
use soroban_sdk::{
    contract, contractclient, contractimpl, token::TokenClient, Address, Bytes, Env,
};
use storage::{get_fee_state, get_pair_state, set_fee_state, set_pair_state};

#[contractclient(name = "LpTokenClient")]
pub trait LpTokenInterface {
    fn mint(env: Env, to: Address, amount: i128);
    fn total_supply(env: Env) -> i128;
}

#[contract]
pub struct Pair;

#[contractimpl]
impl Pair {
    // ── Initialization ────────────────────────────────────────────────────────

    /// Initializes a new liquidity pair with two tokens and an LP token.
    ///
    /// Sets up the pair contract with initial configuration. This function must be called
    /// exactly once after the contract is deployed. It establishes the relationship between
    /// two ERC-20 tokens and their corresponding LP token.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `factory` - The address of the factory contract that created this pair
    /// * `token_a` - The address of the first token in the pair
    /// * `token_b` - The address of the second token in the pair
    /// * `lp_token` - The address of the LP (liquidity provider) token
    ///
    /// # Returns
    /// * `Ok(())` - If initialization was successful
    /// * `Err(PairError::AlreadyInitialized)` - If the pair has already been initialized
    ///
    /// # Panics
    /// * If `factory`, `token_a`, `token_b`, or `lp_token` addresses are invalid
    ///
    /// # Example
    /// ```ignore
    /// let result = Pair::initialize(
    ///     env,
    ///     factory_address,
    ///     token_a_address,
    ///     token_b_address,
    ///     lp_token_address,
    /// );
    /// assert_eq!(result, Ok(()));
    /// ```
    pub fn initialize(
        env: Env,
        factory: Address,
        token_a: Address,
        token_b: Address,
        lp_token: Address,
    ) -> Result<(), PairError> {
        if get_pair_state(&env).is_some() {
            return Err(PairError::AlreadyInitialized);
        }
        let state = storage::PairStorage {
            factory,
            token_a,
            token_b,
            lp_token,
            reserve_a: 0,
            reserve_b: 0,
            block_timestamp_last: env.ledger().timestamp(),
            price_a_cumulative: 0,
            price_b_cumulative: 0,
            k_last: 0,
        };
        set_pair_state(&env, &state);
        Ok(())
    }

    /// Mints LP tokens when liquidity is deposited into the pair.
    ///
    /// The caller must have already transferred both token_a and token_b to this contract
    /// in amounts greater than the current reserves. The difference between transferred
    /// amounts and current reserves is treated as the liquidity contribution. The
    /// corresponding amount of LP tokens is minted to the `to` address.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `to` - The recipient address for the minted LP tokens
    ///
    /// # Returns
    /// * `Ok(liquidity)` - The amount of LP tokens minted
    /// * `Err(PairError::NotInitialized)` - If the pair has not been initialized
    /// * `Err(PairError::InsufficientLiquidityMinted)` - If the computed liquidity amount is zero or negative
    /// * `Err(PairError::Overflow)` - If arithmetic operations overflow
    ///
    /// # Panics
    /// * If authentication from `to` address fails
    /// * If token balance queries fail
    /// * If LP token minting transaction fails
    ///
    /// # Example
    /// ```ignore
    /// // After transferring tokens to the pair contract
    /// let lp_amount = Pair::mint(env, recipient_address)?;
    /// println!("Minted {} LP tokens", lp_amount);
    /// ```
    pub fn mint(env: Env, to: Address) -> Result<i128, PairError> {
        to.require_auth();

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let contract = env.current_contract_address();

        let balance_a = TokenClient::new(&env, &state.token_a).balance(&contract);
        let balance_b = TokenClient::new(&env, &state.token_b).balance(&contract);
        let amount_a = balance_a - state.reserve_a;
        let amount_b = balance_b - state.reserve_b;

        let lp_client = LpTokenClient::new(&env, &state.lp_token);
        let total_supply = lp_client.total_supply();

        let liquidity;
        if total_supply == 0 {
            liquidity = math::sqrt(amount_a.checked_mul(amount_b).ok_or(PairError::Overflow)?)
                - MINIMUM_LIQUIDITY;
            if liquidity <= 0 {
                return Err(PairError::InsufficientLiquidityMinted);
            }
            lp_client.mint(&contract, &MINIMUM_LIQUIDITY);
        } else {
            let liquidity_a =
                amount_a.checked_mul(total_supply).ok_or(PairError::Overflow)? / state.reserve_a;
            let liquidity_b =
                amount_b.checked_mul(total_supply).ok_or(PairError::Overflow)? / state.reserve_b;
            liquidity = liquidity_a.min(liquidity_b);
        }

        if liquidity <= 0 {
            return Err(PairError::InsufficientLiquidityMinted);
        }

        lp_client.mint(&to, &liquidity);

        state.reserve_a = balance_a;
        state.reserve_b = balance_b;
        state.k_last = balance_a.checked_mul(balance_b).ok_or(PairError::Overflow)?;
        set_pair_state(&env, &state);

        PairEvents::mint(&env, &to, amount_a, amount_b);

        Ok(liquidity)
    }

    /// Burns LP tokens and returns the underlying liquidity (both tokens).
    ///
    /// The caller must have already transferred LP tokens to this contract. The LP token
    /// balance in the contract is burned, and the proportional amounts of both token_a
    /// and token_b are transferred to the `to` address.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `to` - The recipient address for the withdrawn tokens
    ///
    /// # Returns
    /// * `Ok((amount_a, amount_b))` - The amounts of token_a and token_b returned
    /// * `Err(PairError::NotInitialized)` - If the pair has not been initialized
    /// * `Err(PairError::InsufficientLiquidityBurned)` - If computed amounts are zero or negative
    /// * `Err(PairError::Overflow)` - If arithmetic operations overflow
    ///
    /// # Panics
    /// * If authentication from `to` address fails
    /// * If token operations (burn, transfer) fail
    /// * If reserve update fails
    ///
    /// # Example
    /// ```ignore
    /// // After transferring LP tokens to the pair contract
    /// let (amount_a, amount_b) = Pair::burn(env, recipient_address)?;
    /// println!("Received {} token_a and {} token_b", amount_a, amount_b);
    /// ```
    pub fn burn(env: Env, to: Address) -> Result<(i128, i128), PairError> {
        to.require_auth();

        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let contract = env.current_contract_address();

        let lp_balance = TokenClient::new(&env, &state.lp_token).balance(&contract);
        let total_supply = LpTokenClient::new(&env, &state.lp_token).total_supply();

        let amount_a =
            lp_balance.checked_mul(state.reserve_a).ok_or(PairError::Overflow)? / total_supply;
        let amount_b =
            lp_balance.checked_mul(state.reserve_b).ok_or(PairError::Overflow)? / total_supply;

        if amount_a <= 0 || amount_b <= 0 {
            return Err(PairError::InsufficientLiquidityBurned);
        }

        TokenClient::new(&env, &state.lp_token).burn(&contract, &lp_balance);

        TokenClient::new(&env, &state.token_a).transfer(&contract, &to, &amount_a);
        TokenClient::new(&env, &state.token_b).transfer(&contract, &to, &amount_b);

        state.reserve_a -= amount_a;
        state.reserve_b -= amount_b;
        state.k_last = state.reserve_a.checked_mul(state.reserve_b).ok_or(PairError::Overflow)?;
        set_pair_state(&env, &state);

        PairEvents::burn(&env, &to, amount_a, amount_b, &to);

        Ok((amount_a, amount_b))
    }

    // ── Swap ──────────────────────────────────────────────────────────────────

    /// Executes a constant-product swap with dynamic fees and reentrancy protection.
    ///
    /// Performs an atomic token swap using the constant-product formula (x·y=k). The caller
    /// must have transferred the input tokens to this contract before calling (Uniswap V2 pattern).
    /// This function includes dynamic fee calculation based on market volatility, and verifies
    /// that the k-invariant is maintained.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `amount_a_out` - Amount of token_a to receive (0 if swapping B→A)
    /// * `amount_b_out` - Amount of token_b to receive (0 if swapping A→B)
    /// * `to` - Recipient address for the output tokens
    ///
    /// # Returns
    /// * `Ok(())` - If the swap executed successfully
    /// * `Err(PairError::NotInitialized)` - If the pair has not been initialized
    /// * `Err(PairError::InsufficientOutputAmount)` - If both output amounts are zero or result is invalid
    /// * `Err(PairError::InsufficientLiquidity)` - If requested output exceeds available reserves
    /// * `Err(PairError::InsufficientInputAmount)` - If no input tokens were transferred
    /// * `Err(PairError::InvalidK)` - If the k-invariant check fails after fee application
    /// * `Err(PairError::Overflow)` - If arithmetic operations overflow
    ///
    /// # Panics
    /// * If reentrancy protection fails (concurrent swap attempted)
    /// * If token transfer operations fail
    /// * If reserve or fee state updates fail
    ///
    /// # Example
    /// ```ignore
    /// // After transferring 100 units of token_a to the pair contract
    /// let result = Pair::swap(env, 0, 50, recipient_address)?;
    /// // Swap 100 token_a for 50 token_b
    /// ```
    pub fn swap(
        env: Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: Address,
    ) -> Result<(), PairError> {
        // ── 1. Reentrancy guard ───────────────────────────────────────────────
        reentrancy::acquire(&env)?;

        let result = Self::swap_inner(&env, amount_a_out, amount_b_out, &to);

        // Always release guard, even on error.
        reentrancy::release(&env);

        result
    }

    fn swap_inner(
        env: &Env,
        amount_a_out: i128,
        amount_b_out: i128,
        to: &Address,
    ) -> Result<(), PairError> {
        // ── 2. Input validation ───────────────────────────────────────────────
        if amount_a_out <= 0 && amount_b_out <= 0 {
            return Err(PairError::InsufficientOutputAmount);
        }

        // ── 3. Load state ─────────────────────────────────────────────────────
        let mut pair = get_pair_state(env).ok_or(PairError::NotInitialized)?;
        let mut fee_state = get_fee_state(env).ok_or(PairError::NotInitialized)?;

        // ── 4. Check output vs reserves ───────────────────────────────────────
        if amount_a_out >= pair.reserve_a || amount_b_out >= pair.reserve_b {
            return Err(PairError::InsufficientLiquidity);
        }

        // ── 5. Decay stale fee before computing ───────────────────────────────
        dynamic_fee::decay_stale_ema(env, &mut fee_state);

        // ── 6. Compute fee ───────────────────────────────────────────────────
        let fee_bps = dynamic_fee::compute_fee_bps(&fee_state);

        // ── 7. Optimistic transfer: send output tokens to recipient ───────────
        let contract_address = env.current_contract_address();

        if amount_a_out > 0 {
            TokenClient::new(env, &pair.token_a).transfer(&contract_address, to, &amount_a_out);
        }
        if amount_b_out > 0 {
            TokenClient::new(env, &pair.token_b).transfer(&contract_address, to, &amount_b_out);
        }

        // ── 8. Read actual balances post-transfer ───────────────────────────
        let balance_a = TokenClient::new(env, &pair.token_a).balance(&contract_address);
        let balance_b = TokenClient::new(env, &pair.token_b).balance(&contract_address);

        // ── 9. Compute effective amounts in ───────────────────────────────────
        // amount_in = new_balance - (old_reserve - amount_out), floored at 0
        let amount_a_in = (balance_a - (pair.reserve_a - amount_a_out)).max(0);
        let amount_b_in = (balance_b - (pair.reserve_b - amount_b_out)).max(0);

        if amount_a_in <= 0 && amount_b_in <= 0 {
            return Err(PairError::InsufficientInputAmount);
        }

        // ── 10. Fee-adjusted balances (Uniswap V2 K check) ───────────────────
        // balance_adjusted = balance * 10_000 - amount_in * fee_bps
        // This avoids floating point: multiply reserves by 10_000 so fee
        // subtraction is exact.
        let fee = fee_bps as i128;
        let balance_a_adj = balance_a
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_sub(amount_a_in * fee)
            .ok_or(PairError::Overflow)?;
        let balance_b_adj = balance_b
            .checked_mul(10_000)
            .ok_or(PairError::Overflow)?
            .checked_sub(amount_b_in * fee)
            .ok_or(PairError::Overflow)?;

        if balance_a_adj <= 0 || balance_b_adj <= 0 {
            return Err(PairError::InsufficientOutputAmount);
        }

        // ── 11. K-invariant check ─────────────────────────────────────────────
        // balance_a_adj * balance_b_adj >= reserve_a * reserve_b * 10_000^2
        let k_before = pair
            .reserve_a
            .checked_mul(pair.reserve_b)
            .ok_or(PairError::Overflow)?
            .checked_mul(100_000_000) // 10_000^2
            .ok_or(PairError::Overflow)?;

        let k_after = balance_a_adj.checked_mul(balance_b_adj).ok_or(PairError::Overflow)?;

        if k_after < k_before {
            return Err(PairError::InvalidK);
        }

        // ── 12. Update volatility EMA ─────────────────────────────────────────
        // Price delta: |reserve_b/reserve_a - new_balance_b/new_balance_a|
        // Approximate with integer arithmetic.
        let total_reserve = pair.reserve_a.saturating_add(pair.reserve_b);
        let trade_size = amount_a_in.max(amount_b_in);
        // Simple price delta proxy: change in effective reserve ratio.
        let old_price =
            if pair.reserve_a > 0 { (pair.reserve_b * 10_000) / pair.reserve_a } else { 0 };
        let new_price = if balance_a > 0 { (balance_b * 10_000) / balance_a } else { 0 };
        let price_delta = (new_price - old_price).unsigned_abs() as i128;

        dynamic_fee::update_volatility(
            env,
            &mut fee_state,
            price_delta,
            trade_size,
            total_reserve,
        )?;

        // ── 13. Update K_last and reserves ────────────────────────────────────
        pair.k_last = balance_a * balance_b;
        pair.reserve_a = balance_a;
        pair.reserve_b = balance_b;
        pair.block_timestamp_last = env.ledger().timestamp();

        // ── 14. Persist state ─────────────────────────────────────────────────
        set_pair_state(env, &pair);
        set_fee_state(env, &fee_state);

        // ── 15. Emit swap event ───────────────────────────────────────────────
        // sender = invoker (the caller who initiated this swap)
        let sender = to; // conservative: use `to` as event sender proxy
        PairEvents::swap(
            env,
            sender,
            amount_a_in,
            amount_b_in,
            amount_a_out,
            amount_b_out,
            fee_bps,
            to,
        );

        Ok(())
    }

    // Flash Loan ────────────────────────────────────────────────────────────

    /// Executes an uncollateralized flash loan of tokens to a receiver contract.
    ///
    /// Sends up to `amount_a` of token_a and/or `amount_b` of token_b to the receiver
    /// contract. The receiver must implement the flash loan callback interface and must
    /// return (repay) the borrowed amount plus a fee before the callback completes.
    /// This function includes reentrancy protection to prevent double-borrowing.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `receiver` - The address of the contract receiving the flash loan (must implement callback)
    /// * `amount_a` - Amount of token_a to borrow (0 if not needed)
    /// * `amount_b` - Amount of token_b to borrow (0 if not needed)
    /// * `data` - Arbitrary data passed to the receiver's callback function
    ///
    /// # Returns
    /// * `Ok(())` - If the flash loan was executed and repaid successfully
    /// * `Err(PairError::NotInitialized)` - If the pair has not been initialized
    /// * `Err(PairError::InsufficientLiquidity)` - If requested amounts exceed available reserves
    /// * `Err(PairError::InvalidK)` - If reserves are invalid after repayment
    /// * `Err(PairError::Overflow)` - If fee calculations overflow
    ///
    /// # Panics
    /// * If reentrancy detection identifies a concurrent flash loan
    /// * If token transfer operations fail
    /// * If the receiver's callback fails or returns insufficient repayment
    /// * If reserve updates fail
    ///
    /// # Example
    /// ```ignore
    /// let result = Pair::flash_loan(
    ///     env,
    ///     receiver_contract_address,
    ///     1000,  // Borrow 1000 of token_a
    ///     0,     // Don't borrow token_b
    ///     Bytes::new(&env, b"custom_data"),
    /// )?;
    /// ```
    pub fn flash_loan(
        env: Env,
        receiver: Address,
        amount_a: i128,
        amount_b: i128,
        data: Bytes,
    ) -> Result<(), PairError> {
        flash_loan::execute_flash_loan(&env, &receiver, amount_a, amount_b, &data)
    }

    /// Returns the current reserves and block timestamp of the pair.
    ///
    /// Retrieves the current amounts of both tokens held by the pair contract and the
    /// timestamp of the last state-modifying operation. These values are used by external
    /// contracts and off-chain systems for price calculations and liquidity information.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// A tuple of `(reserve_a, reserve_b, block_timestamp_last)` where:
    /// * `reserve_a` - Current amount of token_a in the pair
    /// * `reserve_b` - Current amount of token_b in the pair
    /// * `block_timestamp_last` - The ledger timestamp of the last update
    ///
    /// # Panics
    /// * If the pair has not been initialized (falls back to (0, 0, 0) with unwrap)
    ///
    /// # Example
    /// ```ignore
    /// let (reserve_a, reserve_b, last_timestamp) = Pair::get_reserves(env);
    /// println!("Reserves: {} token_a, {} token_b at timestamp {}",
    ///     reserve_a, reserve_b, last_timestamp);
    /// ```
    pub fn get_reserves(env: Env) -> (i128, i128, u64) {
        let state = get_pair_state(&env).ok_or(PairError::NotInitialized).unwrap();
        (state.reserve_a, state.reserve_b, state.block_timestamp_last)
    }

    /// Returns the current dynamic fee in basis points.
    ///
    /// Calculates and returns the current swap fee based on recent market volatility.
    /// The fee adjusts dynamically:higher volatility results in higher fees, providing
    /// protection against slippage and MEV. A basis point (bps) is 1/100th of a percent,
    /// so a fee of 30 bps equals 0.3%.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current swap fee in basis points (e.g., 30 for 0.3%)
    ///
    /// # Example
    /// ```ignore
    /// let fee_bps = Pair::get_current_fee_bps(env);
    /// let fee_percent = fee_bps as f64 / 100.0;
    /// println!("Current swap fee: {} bps ({:.2}%)", fee_bps, fee_percent);
    /// ```
    pub fn get_current_fee_bps(env: Env) -> u32 {
        get_fee_state(&env).map(|fs| dynamic_fee::compute_fee_bps(&fs)).unwrap_or(30)
    }

    pub fn lp_token(env: Env) -> Result<Address, PairError> {
        let state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        Ok(state.lp_token)
    }

    /// Synchronizes the pair's internal reserves with actual token balances.
    ///
    /// Updates the stored reserves to match the actual token balances currently held by
    /// the pair contract. This is useful for emergency recovery if reserves become
    /// desynchronized due to direct token transfers or other exceptional circumstances.
    /// Should only be called when necessary, as it bypasses normal state update logic.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// * `Ok(())` - If reserves were successfully synchronized
    /// * `Err(PairError::NotInitialized)` - If the pair has not been initialized
    ///
    /// # Panics
    /// * If token balance queries fail
    /// * If reserve update transaction fails
    /// * If event emission fails
    ///
    /// # Example
    /// ```ignore
    /// // Resynchronize reserves after an anomalous transaction
    /// let result = Pair::sync(env)?;
    /// println!("Reserves synchronized");
    /// ```
    pub fn sync(env: Env) -> Result<(), PairError> {
        let mut state = get_pair_state(&env).ok_or(PairError::NotInitialized)?;
        let contract = env.current_contract_address();
        let balance_a = TokenClient::new(&env, &state.token_a).balance(&contract);
        let balance_b = TokenClient::new(&env, &state.token_b).balance(&contract);

        // ── Update cumulative price accumulators ──────────────────────────────
        let current_timestamp = env.ledger().timestamp();
        let time_elapsed = current_timestamp.saturating_sub(state.block_timestamp_last) as i128;

        if time_elapsed > 0 && state.reserve_a > 0 && state.reserve_b > 0 {
            // price_a_cumulative += (reserve_b / reserve_a) * time_elapsed
            // Using integer division: (reserve_b * time_elapsed) / reserve_a
            let price_a_delta = state
                .reserve_b
                .checked_mul(time_elapsed)
                .ok_or(PairError::Overflow)?
                .checked_div(state.reserve_a)
                .ok_or(PairError::Overflow)?;
            state.price_a_cumulative = state.price_a_cumulative.checked_add(price_a_delta).ok_or(PairError::Overflow)?;

            // price_b_cumulative += (reserve_a / reserve_b) * time_elapsed
            // Using integer division: (reserve_a * time_elapsed) / reserve_b
            let price_b_delta = state
                .reserve_a
                .checked_mul(time_elapsed)
                .ok_or(PairError::Overflow)?
                .checked_div(state.reserve_b)
                .ok_or(PairError::Overflow)?;
            state.price_b_cumulative = state.price_b_cumulative.checked_add(price_b_delta).ok_or(PairError::Overflow)?;
        }

        // ── Update reserves and timestamp ────────────────────────────────────
        state.reserve_a = balance_a;
        state.reserve_b = balance_b;
        state.block_timestamp_last = current_timestamp;
        set_pair_state(&env, &state);
        PairEvents::sync(&env, balance_a, balance_b);
        Ok(())
    }
}
