use launcher_core::application::account_capes::account_capes as capes;
use launcher_core::application::account_skin::account_skin as skin;
use launcher_core::application::change_skin::change_skin as upload;
use launcher_core::application::check_premium_name::check_premium_name as check;
use launcher_core::application::create_offline_profile::create_offline_profile;
use launcher_core::application::list_accounts::list_accounts as list;
use launcher_core::application::login_microsoft::{LoginDeps, login_microsoft as login};
use launcher_core::application::remove_account::remove_account as remove;
use launcher_core::application::select_account::{SelectAccountError, select_account};
use launcher_core::application::set_cape::set_cape as wear;
use launcher_core::domain::account::{CapeId, MAX_SKIN_PNG_BYTES, SkinVariant};
use tauri::State;
use tauri::ipc::Channel;

use super::parse_account_id;
use crate::dto::{
    AccountDto, AccountPolicyDto, AccountsDto, CapeDto, DeviceCodeDto, PremiumNameDto, SkinDto,
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

/// Uploads a skin (`pngBase64`, `variant` "classic" or "slim") for a Microsoft
/// account. Sent once: never retried.
#[tauri::command]
pub async fn change_account_skin(
    state: State<'_, AppState>,
    account_id: String,
    png_base64: String,
    variant: String,
) -> Result<(), AppError> {
    use base64::Engine;
    let id = parse_account_id(&account_id)?;
    let variant = match variant.as_str() {
        "classic" => SkinVariant::Classic,
        "slim" => SkinVariant::Slim,
        _ => {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "unknown skin variant",
            ));
        }
    };
    // Base64 grows data by 4/3; refuse anything that cannot decode to a skin.
    if png_base64.len() > MAX_SKIN_PNG_BYTES / 3 * 4 + 4 {
        return Err(AppError::new(
            ErrorCode::InvalidSkin,
            "the image is too large",
        ));
    }
    let png = base64::engine::general_purpose::STANDARD
        .decode(png_base64.as_bytes())
        .map_err(|_| AppError::new(ErrorCode::InvalidSkin, "the image is not valid base64"))?;
    let auth = state.msa.as_ref().ok_or_else(not_configured)?;
    Ok(upload(id, png, variant, &state.accounts, &state.sessions(), auth).await?)
}

/// The capes a Microsoft account owns, read live from Mojang.
#[tauri::command]
pub async fn list_account_capes(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<CapeDto>, AppError> {
    let id = parse_account_id(&account_id)?;
    let auth = state.msa.as_ref().ok_or_else(not_configured)?;
    let capes = capes(id, &state.accounts, &state.sessions(), auth).await?;
    Ok(capes.iter().map(CapeDto::from).collect())
}

/// Wears an owned cape (`capeId`), or none for `null`.
#[tauri::command]
pub async fn set_account_cape(
    state: State<'_, AppState>,
    account_id: String,
    cape_id: Option<String>,
) -> Result<(), AppError> {
    let id = parse_account_id(&account_id)?;
    let cape = cape_id
        .map(|raw| CapeId::parse(&raw))
        .transpose()
        .map_err(|error| AppError::new(ErrorCode::InvalidInput, error.to_string()))?;
    let auth = state.msa.as_ref().ok_or_else(not_configured)?;
    Ok(wear(id, cape, &state.accounts, &state.sessions(), auth).await?)
}

fn not_configured() -> AppError {
    AppError::new(
        ErrorCode::MicrosoftNotConfigured,
        "this build has no Microsoft sign-in (MSA_CLIENT_ID)",
    )
}
