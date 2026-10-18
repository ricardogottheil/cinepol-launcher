use launcher_core::application::check_premium_name::check_premium_name as check;
use launcher_core::application::create_offline_profile::create_offline_profile;
use launcher_core::application::list_accounts::list_accounts as list;
use launcher_core::application::select_account::{SelectAccountError, select_account};
use tauri::State;

use super::parse_account_id;
use crate::dto::{AccountDto, AccountsDto, PremiumNameDto};
use crate::error::{AppError, ErrorCode};
use crate::state::AppState;

#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<AccountsDto, AppError> {
    let (accounts, active) = list(&state.accounts).await?;
    Ok(AccountsDto {
        accounts: accounts.iter().map(AccountDto::from).collect(),
        active_id: active.map(|id| id.as_uuid().to_string()),
    })
}

#[tauri::command]
pub async fn create_offline_account(
    state: State<'_, AppState>,
    username: String,
) -> Result<AccountDto, AppError> {
    let account = create_offline_profile(&username, state.offline_policy, &state.accounts).await?;
    Ok(AccountDto::from(&account))
}

/// Whether a premium account owns the name; `unknown` when the lookup failed.
#[tauri::command]
pub async fn check_premium_name(
    state: State<'_, AppState>,
    username: String,
) -> Result<PremiumNameDto, AppError> {
    let result = check(&username, &state.premium_names)
        .await
        .map_err(|error| AppError::new(ErrorCode::InvalidInput, error.to_string()))?;
    Ok(PremiumNameDto::from(result))
}

#[tauri::command]
pub async fn set_active_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), AppError> {
    let id = parse_account_id(&account_id)?;
    select_account(id, &state.accounts)
        .await
        .map_err(|error| match error {
            SelectAccountError::NotFound => AppError::new(ErrorCode::NotFound, error.to_string()),
            SelectAccountError::Storage(storage) => AppError::from(storage),
        })
}
