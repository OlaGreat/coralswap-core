use crate::errors::LpTokenError;
use crate::{LpToken, LpTokenClient};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

fn setup_env() -> (Env, Address, LpTokenClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, LpToken);
    let client = LpTokenClient::new(&env, &contract_id);
    let admin = Address::generate(&env);

    (env, contract_id, client, admin)
}

fn initialize_client(client: &LpTokenClient<'_>, env: &Env, admin: &Address) {
    client.initialize(admin, &7, &String::from_str(env, "Coral LP"), &String::from_str(env, "CLP"));
}

#[test]
fn decimals_returns_not_initialized_when_metadata_missing() {
    let (_env, _contract_id, client, _admin) = setup_env();

    let result = client.try_decimals();
    assert_eq!(result, Err(Ok(LpTokenError::NotInitialized)));
}

#[test]
fn name_returns_not_initialized_when_metadata_missing() {
    let (_env, _contract_id, client, _admin) = setup_env();

    let result = client.try_name();
    assert_eq!(result, Err(Ok(LpTokenError::NotInitialized)));
}

#[test]
fn symbol_returns_not_initialized_when_metadata_missing() {
    let (_env, _contract_id, client, _admin) = setup_env();

    let result = client.try_symbol();
    assert_eq!(result, Err(Ok(LpTokenError::NotInitialized)));
}

#[test]
fn metadata_reads_return_stored_values_after_initialize() {
    let (env, _contract_id, client, admin) = setup_env();

    initialize_client(&client, &env, &admin);

    assert_eq!(client.decimals(), 7);
    assert_eq!(client.name(), String::from_str(&env, "Coral LP"));
    assert_eq!(client.symbol(), String::from_str(&env, "CLP"));
}
