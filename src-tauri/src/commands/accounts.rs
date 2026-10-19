use launcher_core::application::account_skin::account_skin as skin;
use launcher_core::application::check_premium_name::check_premium_name as check;
use launcher_core::application::create_offline_profile::create_offline_profile;
use launcher_core::application::list_accounts::list_accounts as list;
use launcher_core::application::login_microsoft::{LoginDeps, login_microsoft as login};
use launcher_core::application::remove_account::remove_account as remove;
use launcher_core::application::select_account::{SelectAccountError, select_account};
use tauri::State;
use tauri::ipc::Channel;

use super::parse_account_id;
use crate::dto::{
    AccountDto, AccountPolicyDto, AccountsDto, DeviceCodeDto, PremiumNameDto, SkinDto,
};
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

/// What the Accounts screen may offer in this build.
#[tauri::command]
pub fn get_account_policy(state: State<'_, AppState>) -> AccountPolicyDto {
    AccountPolicyDto {
        microsoft_available: state.msa.is_some(),
        require_premium_for_offline: state.offline_policy.require_premium_for_offline,
    }
}

/// Signs in with Microsoft. The code to show arrives on `on_code`; the
/// sign-in is cancellable with `cancel_operation`.
#[tauri::command]
pub async fn login_microsoft(
    state: State<'_, AppState>,
    operation_id: String,
    on_code: Channel<DeviceCodeDto>,
) -> Result<AccountDto, AppError> {
    let auth = state.msa.as_ref().ok_or_else(not_configured)?;
    let token = state.operations.start(&operation_id)?;
    let deps = LoginDeps {
        auth,
        accounts: &state.accounts,
        secrets: &state.secrets,
        clock: &state.clock,
        delay: &state.delay,
    };
    let result = login(
        &deps,
        |code| {
            let _ = on_code.send(DeviceCodeDto::from(code));
        },
        &token,
    )
    .await;
    state.operations.finish(&operation_id);
    Ok(AccountDto::from(&result?))
}

/// Removes an account and its tokens; instances pinned to it use the active account.
#[tauri::command]
pub async fn remove_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), AppError> {
    let id = parse_account_id(&account_id)?;
    Ok(remove(id, &state.accounts, &state.secrets, &state.instances).await?)
}

/// The account's skin for the 3D preview; `null` when it has none.
#[tauri::command]
pub async fn get_account_skin(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<SkinDto>, AppError> {
    let id = parse_account_id(&account_id)?;
    let Some(auth) = &state.msa else {
        return Ok(None);
    };
    Ok(skin(id, &state.accounts, auth)
        .await?
        .as_ref()
        .map(SkinDto::from))
}

fn not_configured() -> AppError {
    AppError::new(
        ErrorCode::MicrosoftNotConfigured,
        "this build has no Microsoft sign-in (MSA_CLIENT_ID)",
    )
}
