use crate::models::{ExtraUsageInfo, ProviderRateLimits, RateLimitWindow};
use chrono::Local;
use serde::Deserialize;
use std::path::Path;
use std::sync::Mutex;

use super::http::rate_limit_error_from_response;
use super::RateLimitFetchError;

const ANTHROPIC_USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const ANTHROPIC_ACCOUNT_URL: &str = "https://api.anthropic.com/api/oauth/account";

/// In-process cache of the Claude OAuth access token.
///
/// Claude Code rewrites the `Claude Code-credentials` Keychain item each time
/// it rotates its OAuth token. That rewrite resets the item's ACL / partition
/// list, so the user's "Always Allow" grant for TokenMonitor is lost — and
/// without a cache the next background refresh (every ~2.5 min) re-prompts.
/// Caching lets us reuse the token across refresh cycles and only touch the
/// Keychain on a cold cache or when the API returns 401 (real rotation).
static CACHED_ACCESS_TOKEN: Mutex<Option<String>> = Mutex::new(None);

fn cached_access_token() -> Option<String> {
    CACHED_ACCESS_TOKEN.lock().ok().and_then(|g| g.clone())
}

fn store_access_token(token: &str) {
    if let Ok(mut guard) = CACHED_ACCESS_TOKEN.lock() {
        *guard = Some(token.to_string());
    }
}

fn invalidate_access_token_cache() {
    if let Ok(mut guard) = CACHED_ACCESS_TOKEN.lock() {
        *guard = None;
    }
}

/// Extract `claudeAiOauth.accessToken` from a JSON string.
fn extract_access_token(json_str: &str) -> Result<String, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str.trim()).map_err(|e| format!("Invalid JSON: {e}"))?;

    parsed
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No claudeAiOauth.accessToken in credentials".to_string())
}

/// Extract `claudeAiOauth.refreshToken` from a JSON string. Used by the
/// OAuth refresh-grant flow to mint a fresh access token without prompting
/// the user. macOS-only because the owned-mirror refresh path is too —
/// Linux/Windows currently rely on `~/.claude/.credentials.json` instead.
#[cfg(target_os = "macos")]
fn extract_refresh_token(json_str: &str) -> Result<String, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str.trim()).map_err(|e| format!("Invalid JSON: {e}"))?;

    parsed
        .get("claudeAiOauth")
        .and_then(|o| o.get("refreshToken"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No claudeAiOauth.refreshToken in credentials".to_string())
}

/// Patch the credentials JSON with a refreshed access token, optionally
/// rotating the refresh token + expiresAt as well. Preserves every other
/// field so the mirror keeps its `subscriptionType`, `rateLimitTier`, MCP
/// OAuth state, etc. macOS-only — the only caller is
/// [`try_refresh_via_owned_mirror`].
#[cfg(target_os = "macos")]
fn update_credentials_with_refresh(
    original_json: &str,
    new_access_token: &str,
    new_refresh_token: Option<&str>,
    expires_in_secs: Option<u64>,
) -> Result<String, String> {
    let mut parsed: serde_json::Value =
        serde_json::from_str(original_json.trim()).map_err(|e| format!("Invalid JSON: {e}"))?;

    let oauth = parsed
        .get_mut("claudeAiOauth")
        .ok_or_else(|| "Missing claudeAiOauth root".to_string())?
        .as_object_mut()
        .ok_or_else(|| "claudeAiOauth is not an object".to_string())?;

    oauth.insert(
        "accessToken".to_string(),
        serde_json::Value::String(new_access_token.to_string()),
    );
    if let Some(refresh) = new_refresh_token {
        oauth.insert(
            "refreshToken".to_string(),
            serde_json::Value::String(refresh.to_string()),
        );
    }
    if let Some(secs) = expires_in_secs {
        let expires_at_ms =
            (chrono::Utc::now().timestamp_millis() as u64) + secs.saturating_mul(1000);
        oauth.insert(
            "expiresAt".to_string(),
            serde_json::Value::Number(serde_json::Number::from(expires_at_ms)),
        );
    }

    serde_json::to_string(&parsed).map_err(|e| format!("Failed to serialize updated JSON: {e}"))
}

/// macOS-only: TokenMonitor's own Keychain item that mirrors the Claude Code
/// credentials JSON. Created on every successful interactive read in
/// [`prime_token_from_keychain_interactive`]. The default ACL set by
/// `set_generic_password` restricts access to the calling code-signing
/// identity (TokenMonitor), so unlike Claude Code's item this one survives
/// Claude Code's token rotations — Claude Code never writes here, so its
/// rotations cannot reset our ACL.
#[cfg(target_os = "macos")]
const OWNED_KEYCHAIN_SERVICE: &str = "com.tokenmonitor.app.claude-oauth";
#[cfg(target_os = "macos")]
const OWNED_KEYCHAIN_ACCOUNT: &str = "default";

/// Write the full Claude Code credentials JSON into TokenMonitor's owned
/// Keychain item.
///
/// Uses the **legacy** `SecKeychain::set_generic_password` rather than the
/// unified `passwords::set_generic_password` because the unified API uses
/// `SecItemAdd`, which (on the macOS file-keychain) creates items with an
/// **empty trusted-apps list** — so every subsequent silent read fails
/// with `errSecAuthFailed` even from the same process that wrote it.
/// `SecKeychainAddGenericPassword` (legacy) registers the calling app's
/// code-signing identity in the ACL, which is what we need for silent
/// read-back. Without this, the mirror is effectively write-only.
#[cfg(target_os = "macos")]
fn write_credentials_to_owned_keychain(credentials_json: &str) -> Result<(), String> {
    use security_framework::os::macos::keychain::SecKeychain;

    let keychain =
        SecKeychain::default().map_err(|e| format!("Failed to open default keychain: {e}"))?;
    let result = keychain
        .set_generic_password(
            OWNED_KEYCHAIN_SERVICE,
            OWNED_KEYCHAIN_ACCOUNT,
            credentials_json.as_bytes(),
        )
        .map_err(|e| format!("Failed to write owned Keychain item: {e}"));

    match &result {
        Ok(()) => tracing::info!(
            service = OWNED_KEYCHAIN_SERVICE,
            "Mirrored Claude credentials into owned Keychain item"
        ),
        Err(e) => tracing::warn!(error = %e, "Owned Keychain write failed"),
    }
    result
}

/// Read the access token from TokenMonitor's owned Keychain item. Errors when
/// the item is absent or its payload no longer matches the expected shape.
///
/// User interaction is disabled for the duration of the read. Without that,
/// a fresh dev rebuild (different code-signing identity than the one that
/// wrote the item) would block on a hidden ACL prompt instead of failing
/// fast — which can hang the async refresh loop because this is a sync call.
/// On a real ACL miss we'd rather get a fast error and surface a re-grant
/// banner than wait on UI that may never resolve.
/// Read the raw credentials JSON from the owned mirror. Returns the full
/// payload (access + refresh tokens, expiry, scopes, etc.) so the caller
/// can drive the OAuth refresh flow or write back an updated copy.
///
/// Uses the legacy `find_generic_password` to match the legacy write API
/// — the legacy keychain item we stored has the calling app in its ACL,
/// so this returns the password without prompting (and without falling
/// foul of the `errSecAuthFailed` we'd see from the unified API path).
#[cfg(target_os = "macos")]
fn read_raw_credentials_from_owned_keychain() -> Result<String, String> {
    use security_framework::os::macos::keychain::SecKeychain;
    use security_framework::os::macos::passwords::find_generic_password;

    let _ui_lock = SecKeychain::disable_user_interaction()
        .map_err(|e| format!("Failed to disable Keychain UI for owned read: {e}"))?;

    let (password, _item) =
        find_generic_password(None, OWNED_KEYCHAIN_SERVICE, OWNED_KEYCHAIN_ACCOUNT).map_err(
            |e| {
                // Surface the OSStatus so we can distinguish absent
                // (`-25300`) from ACL-denied (`-25293`).
                let detail = format!("{e}");
                tracing::debug!(error = %detail, "Owned Keychain read failed");
                format!("Owned Keychain item unavailable: {detail}")
            },
        )?;

    let bytes: &[u8] = password.as_ref();
    String::from_utf8(bytes.to_vec())
        .map_err(|e| format!("Invalid UTF-8 in owned Keychain item: {e}"))
}

#[cfg(target_os = "macos")]
fn read_token_from_owned_keychain() -> Result<String, String> {
    let raw = read_raw_credentials_from_owned_keychain()?;
    let token = extract_access_token(&raw)?;
    tracing::debug!(
        prefix = &token[..token.len().min(7)],
        "Owned Keychain read succeeded"
    );
    Ok(token)
}

/// Delete TokenMonitor's owned Keychain item. Called when an API 401 confirms
/// the cached token is stale; the next read will fall through to the silent
/// Claude Code Keychain path or surface an auth error to the user.
#[cfg(target_os = "macos")]
fn delete_owned_keychain_item() {
    if let Err(e) = security_framework::passwords::delete_generic_password(
        OWNED_KEYCHAIN_SERVICE,
        OWNED_KEYCHAIN_ACCOUNT,
    ) {
        // Missing item is the common case — only log other failures.
        let msg = format!("{e}");
        if !msg.contains("-25300") && !msg.to_ascii_lowercase().contains("not found") {
            tracing::debug!(error = %msg, "Failed to delete owned Keychain item");
        }
    }
}

/// Read OAuth token from macOS Keychain via Security.framework.
///
/// Suppressing the Keychain prompt requires **two** mechanisms, because
/// macOS has two keychain stores with different UI-gating knobs:
///
/// 1. `skip_authenticated_items(true)` →
///    `kSecUseAuthenticationUI = kSecUseAuthenticationUISkip`. This governs
///    the **Data Protection keychain** (Touch ID / Face ID items). For
///    those, a would-prompt item is omitted from the result set.
/// 2. `SecKeychain::disable_user_interaction()` →
///    `SecKeychainSetUserInteractionAllowed(false)`. This is a process-wide
///    flag and is the **only** thing that suppresses the classic
///    "Always Allow / Allow / Deny" prompt produced by the **legacy
///    keychain** — which is what `Claude Code-credentials` lives in, since
///    Claude Code writes it through the legacy ACL path. Without this,
///    `kSecUseAuthenticationUISkip` is silently ignored and macOS still
///    pops the ACL panel whenever the machine is awake. (Log evidence for
///    this: reads that failed during dark wake reported "In dark wake, no
///    UI possible" — macOS only emits that after deciding UI was needed,
///    which means the UI-skip flag wasn't consulted.)
///
/// The RAII lock re-enables user interaction on drop, so this function is
/// a pure silent probe: no process-wide side effect outlives the call.
///
/// Claude Code rewrites the credentials item on every OAuth rotation and
/// resets its ACL / partition list, so any "Always Allow" grant the user
/// gave TokenMonitor is dropped with the old item. When the fresh item
/// would prompt, silent denial returns us to the caller, which falls
/// through to the CLI probe in `rate_limits/mod.rs`. That path shells out
/// to the `claude` binary itself — Claude Code is trusted for its own
/// item, so no prompt.
#[cfg(target_os = "macos")]
fn read_token_from_keychain() -> Result<String, String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};
    use security_framework::os::macos::keychain::SecKeychain;

    // Held for the duration of the search; drop re-enables interaction.
    let _ui_lock = SecKeychain::disable_user_interaction()
        .map_err(|e| format!("Failed to disable Keychain UI: {e}"))?;

    let results = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service("Claude Code-credentials")
        .load_data(true)
        .limit(1)
        .skip_authenticated_items(true)
        .search()
        .map_err(|e| format!("Claude Code credentials not available in Keychain: {e}"))?;

    let data = results
        .into_iter()
        .find_map(|r| match r {
            SearchResult::Data(bytes) => Some(bytes),
            _ => None,
        })
        .ok_or_else(|| "Keychain returned no data for Claude Code-credentials".to_string())?;

    let raw = String::from_utf8(data).map_err(|e| format!("Invalid UTF-8 from Keychain: {e}"))?;
    extract_access_token(&raw)
}

/// Interactive Keychain read used by the one-time setup flow.
///
/// Unlike [`read_token_from_keychain`], this deliberately does **not** set
/// `skip_authenticated_items(true)`, so macOS will show the user-auth prompt
/// when needed. This is the only path in the app that allows that prompt to
/// appear — it's invoked from the explicit "Allow Keychain access" button in
/// the welcome flow, never from background refreshes.
///
/// On success the credentials JSON is also copied into TokenMonitor's owned
/// Keychain item ([`write_credentials_to_owned_keychain`]) so future
/// background refreshes can read silently from our own item without depending
/// on Claude Code's ACL surviving the next token rotation. The in-process
/// cache is primed too so the very next API call succeeds without any
/// Keychain round-trip.
#[cfg(target_os = "macos")]
pub(super) fn prime_token_from_keychain_interactive() -> Result<(), String> {
    use security_framework::item::{ItemClass, ItemSearchOptions, SearchResult};

    let results = ItemSearchOptions::new()
        .class(ItemClass::generic_password())
        .service("Claude Code-credentials")
        .load_data(true)
        .limit(1)
        .search()
        .map_err(|e| format!("Keychain access denied or unavailable: {e}"))?;

    let data = results
        .into_iter()
        .find_map(|r| match r {
            SearchResult::Data(bytes) => Some(bytes),
            _ => None,
        })
        .ok_or_else(|| "Claude Code credentials not found in Keychain".to_string())?;

    let raw = String::from_utf8(data).map_err(|e| format!("Invalid UTF-8 from Keychain: {e}"))?;
    let token = extract_access_token(&raw)?;

    // Mirror the credentials into our owned item so we never have to ask
    // Claude Code's Keychain again until this token actually expires. A
    // failure here is non-fatal — we still got the token and can use it for
    // this session.
    if let Err(e) = write_credentials_to_owned_keychain(&raw) {
        tracing::warn!(error = %e, "Failed to mirror credentials into owned Keychain item");
    }

    store_access_token(&token);
    Ok(())
}

fn read_token_from_credentials_path(cred_path: &Path) -> Result<String, String> {
    tracing::debug!(path = %cred_path.display(), "reading file (claude credentials)");
    let raw = std::fs::read_to_string(cred_path)
        .map_err(|e| format!("Failed to read {}: {e}", cred_path.display()))?;

    extract_access_token(&raw)
}

/// Read OAuth token from `~/.claude/.credentials.json`.
///
/// Newer Claude Code builds keep this file current on macOS as well as on
/// Windows/Linux. Prefer it over Keychain because it is a normal file read
/// from the same Claude config directory the app already discloses, so it
/// cannot trigger a macOS Keychain prompt during background refresh.
fn read_token_from_credentials_file() -> Result<String, String> {
    let cred_path = crate::paths::claude_credentials_file()
        .ok_or_else(|| "Cannot determine Claude credentials file path".to_string())?;
    read_token_from_credentials_path(&cred_path)
}

#[cfg(target_os = "macos")]
fn read_token_from_silent_platform_source(credentials_error: String) -> Result<String, String> {
    read_token_from_keychain().map_err(|keychain_error| {
        format!(
            "Claude credentials file unavailable ({credentials_error}); Keychain unavailable ({keychain_error})"
        )
    })
}

#[cfg(not(target_os = "macos"))]
fn read_token_from_silent_platform_source(credentials_error: String) -> Result<String, String> {
    Err(credentials_error)
}

/// Get Claude Code OAuth access token (cross-platform).
///
/// Resolution order:
/// 1. `CLAUDE_CODE_OAUTH_TOKEN` environment variable (JSON string) — never cached
/// 2. In-process cache (set on previous successful read)
/// 3. macOS only: TokenMonitor's owned Keychain item (mirrored from Claude
///    Code's item the last time the user clicked "Allow Keychain access").
///    This is the primary persistent source — it survives Claude Code's
///    token rotations because Claude Code never writes here.
/// 4. `~/.claude/.credentials.json`
/// 5. macOS only: silent read of Claude Code's Keychain item (last-resort
///    fallback when the owned item is missing — typically denied because
///    Claude Code wipes its own item's ACL on rotation).
///
/// On a successful read the token is stored in the in-process cache. Callers
/// that observe a 401 from the API should call
/// [`invalidate_oauth_credentials_after_unauthorized`] so the next call
/// re-reads from a fresh source instead of replaying the stale token.
pub(crate) fn get_claude_oauth_token() -> Result<String, String> {
    // Environment variable override (all platforms). Cheap to read each call,
    // and we don't want to cache an env value that the user might change.
    if let Ok(env_json) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !env_json.trim().is_empty() {
            return extract_access_token(&env_json);
        }
    }

    if let Some(cached) = cached_access_token() {
        return Ok(cached);
    }

    // Owned Keychain item is the persistent source — it survives Claude Code
    // rotations and only goes away when the token genuinely expires (we
    // delete it on 401) or the user revokes via Claude Code logout.
    #[cfg(target_os = "macos")]
    {
        if let Ok(token) = read_token_from_owned_keychain() {
            store_access_token(&token);
            return Ok(token);
        }
    }

    let token =
        read_token_from_credentials_file().or_else(read_token_from_silent_platform_source)?;

    store_access_token(&token);
    Ok(token)
}

/// Invalidate every cached OAuth credential after a confirmed 401 from the
/// usage API. The in-process cache is dropped so the next read goes back to
/// the source, and on macOS the owned Keychain item is deleted because it
/// holds the same expired token as the cache. Without that delete the next
/// read just resurrects the stale token from our own Keychain item and we
/// loop on 401 forever.
///
/// macOS-only — the non-mac retry path calls
/// [`invalidate_access_token_cache`] inline.
#[cfg(target_os = "macos")]
fn invalidate_oauth_credentials_after_unauthorized() {
    invalidate_access_token_cache();
    delete_owned_keychain_item();
}

// ── Claude API response types ──

#[derive(Deserialize)]
pub(crate) struct ClaudeUsageResponse {
    pub five_hour: Option<ClaudeWindowData>,
    pub seven_day: Option<ClaudeWindowData>,
    pub seven_day_sonnet: Option<ClaudeWindowData>,
    pub seven_day_opus: Option<ClaudeWindowData>,
    pub seven_day_oauth_apps: Option<ClaudeWindowData>,
    pub seven_day_cowork: Option<ClaudeWindowData>,
    pub iguana_necktie: Option<ClaudeWindowData>,
    pub extra_usage: Option<ClaudeExtraUsageData>,
}

#[derive(Deserialize)]
pub(crate) struct ClaudeWindowData {
    pub utilization: f64,
    pub resets_at: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct ClaudeExtraUsageData {
    pub is_enabled: bool,
    pub monthly_limit: f64,
    pub used_credits: f64,
    pub utilization: Option<f64>,
}

pub(crate) fn normalize_claude_extra_usage(extra_usage: ClaudeExtraUsageData) -> ExtraUsageInfo {
    ExtraUsageInfo {
        is_enabled: extra_usage.is_enabled,
        // The OAuth usage endpoint reports credit values in cents.
        monthly_limit: extra_usage.monthly_limit / 100.0,
        used_credits: extra_usage.used_credits / 100.0,
        utilization: extra_usage.utilization,
    }
}

#[derive(Deserialize)]
struct ClaudeAccountResponse {
    memberships: Vec<ClaudeMembership>,
}

#[derive(Deserialize)]
struct ClaudeMembership {
    organization: ClaudeOrganization,
}

#[derive(Deserialize)]
struct ClaudeOrganization {
    capabilities: Option<Vec<String>>,
    rate_limit_tier: Option<String>,
}

/// Outcome of one API attempt. We surface 401 separately so the outer
/// function can drop the cached token and retry with a fresh read.
enum FetchAttempt {
    Ok(ProviderRateLimits),
    Unauthorized(RateLimitFetchError),
    Other(RateLimitFetchError),
}

pub(super) async fn fetch_claude_rate_limits() -> Result<ProviderRateLimits, RateLimitFetchError> {
    match try_fetch_claude_rate_limits().await {
        FetchAttempt::Ok(rate_limits) => Ok(rate_limits),
        FetchAttempt::Other(err) => Err(err),
        FetchAttempt::Unauthorized(unauthorized_err) => {
            // Access token is stale. On macOS, first try the OAuth
            // refresh-grant flow with the refresh token already stored in
            // our owned mirror — that survives Anthropic-side rotations
            // without the user re-granting Keychain access. Only on a
            // *confirmed* refresh-token revocation do we delete the mirror
            // and fall back to the interactive prompt path. On
            // Linux/Windows we don't have an owned-mirror flow yet, so we
            // just drop the in-mem cache and retry against the source.
            #[cfg(target_os = "macos")]
            {
                match try_refresh_via_owned_mirror().await {
                    RefreshResult::Refreshed => {
                        tracing::info!("Claude OAuth: refresh succeeded; retrying API call");
                        return match try_fetch_claude_rate_limits().await {
                            FetchAttempt::Ok(rate_limits) => Ok(rate_limits),
                            FetchAttempt::Unauthorized(err) | FetchAttempt::Other(err) => Err(err),
                        };
                    }
                    RefreshResult::Revoked(reason) => {
                        tracing::warn!(reason = %reason,
                            "Claude OAuth: refresh token revoked, deleting owned mirror");
                        invalidate_oauth_credentials_after_unauthorized();
                    }
                    RefreshResult::Transient(reason) => {
                        // Don't delete the mirror on transient refresh
                        // failures — keep it for the next attempt.
                        tracing::warn!(reason = %reason,
                            "Claude OAuth: refresh transient failure, retaining mirror");
                        invalidate_access_token_cache();
                        return Err(unauthorized_err);
                    }
                    RefreshResult::NoMirror => {
                        // No refresh token to use — fall back to the legacy
                        // invalidate-and-retry path (drops in-mem cache,
                        // tries the silent Claude Code Keychain read).
                        invalidate_access_token_cache();
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = unauthorized_err;
                invalidate_access_token_cache();
            }

            match try_fetch_claude_rate_limits().await {
                FetchAttempt::Ok(rate_limits) => Ok(rate_limits),
                FetchAttempt::Unauthorized(err) | FetchAttempt::Other(err) => Err(err),
            }
        }
    }
}

/// Outcome of an attempted refresh-grant against the owned mirror.
#[cfg(target_os = "macos")]
#[derive(Debug)]
enum RefreshResult {
    /// Mirror was refreshed and the new access token is in the in-process
    /// cache, ready for the next API attempt.
    Refreshed,
    /// Anthropic rejected the refresh token. Caller should delete the
    /// mirror and surface re-grant UI.
    Revoked(String),
    /// Network / 5xx / parse error. Mirror is left intact.
    Transient(String),
    /// No mirror exists (or no refresh token in it). Caller should fall
    /// back to the interactive grant path.
    NoMirror,
}

/// Test hook — exposed via `rate_limits::debug_force_refresh` so an IPC
/// can drive the refresh-grant flow without needing a real 401 from
/// Anthropic. Returns a one-line summary suitable for a log line / toast.
#[cfg(target_os = "macos")]
pub(super) async fn debug_force_refresh() -> String {
    match try_refresh_via_owned_mirror().await {
        RefreshResult::Refreshed => "refreshed".to_string(),
        RefreshResult::Revoked(reason) => format!("revoked: {reason}"),
        RefreshResult::Transient(reason) => format!("transient: {reason}"),
        RefreshResult::NoMirror => "no_mirror".to_string(),
    }
}

#[cfg(target_os = "macos")]
async fn try_refresh_via_owned_mirror() -> RefreshResult {
    use super::oauth_refresh::{refresh_oauth_token, RefreshOutcome};

    let raw = match read_raw_credentials_from_owned_keychain() {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(error = %e, "OAuth refresh: no owned mirror to refresh");
            return RefreshResult::NoMirror;
        }
    };
    let refresh_token = match extract_refresh_token(&raw) {
        Ok(t) => t,
        Err(e) => {
            tracing::debug!(error = %e, "OAuth refresh: mirror missing refresh_token");
            return RefreshResult::NoMirror;
        }
    };

    tracing::info!("Claude OAuth: attempting refresh-grant against Anthropic");
    match refresh_oauth_token(&refresh_token).await {
        RefreshOutcome::Refreshed(resp) => {
            // Patch the mirror with the new tokens. Anthropic *may* rotate
            // the refresh token in the response — write whichever one is
            // returned, otherwise keep the existing one.
            let updated = match update_credentials_with_refresh(
                &raw,
                &resp.access_token,
                resp.refresh_token.as_deref(),
                resp.expires_in,
            ) {
                Ok(j) => j,
                Err(e) => return RefreshResult::Transient(format!("rewrite: {e}")),
            };
            if let Err(e) = write_credentials_to_owned_keychain(&updated) {
                return RefreshResult::Transient(format!("mirror write: {e}"));
            }
            invalidate_access_token_cache();
            store_access_token(&resp.access_token);
            RefreshResult::Refreshed
        }
        RefreshOutcome::Revoked(reason) => RefreshResult::Revoked(reason),
        RefreshOutcome::Transient(reason) => RefreshResult::Transient(reason),
    }
}

async fn try_fetch_claude_rate_limits() -> FetchAttempt {
    let token = match get_claude_oauth_token() {
        Ok(token) => token,
        Err(err) => {
            tracing::debug!(reason = %err, "Claude OAuth: no token available");
            return FetchAttempt::Other(RateLimitFetchError::message(err));
        }
    };

    let client = reqwest::Client::new();

    // Fetch usage + account in parallel
    let usage_fut = client
        .get(ANTHROPIC_USAGE_URL)
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send();

    let account_fut = client
        .get(ANTHROPIC_ACCOUNT_URL)
        .bearer_auth(&token)
        .header("anthropic-beta", "oauth-2025-04-20")
        .send();

    let (usage_res, account_res) = tokio::join!(usage_fut, account_fut);

    // Parse usage response
    let usage_resp = match usage_res {
        Ok(r) => r,
        Err(e) => {
            return FetchAttempt::Other(RateLimitFetchError::message(format!(
                "Usage API request failed: {e}"
            )));
        }
    };
    if !usage_resp.status().is_success() {
        let err = rate_limit_error_from_response(&usage_resp);
        return if usage_resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            FetchAttempt::Unauthorized(err)
        } else {
            FetchAttempt::Other(err)
        };
    }
    let usage: ClaudeUsageResponse = match usage_resp.json().await {
        Ok(u) => u,
        Err(e) => {
            return FetchAttempt::Other(RateLimitFetchError::message(format!(
                "Failed to parse usage response: {e}"
            )));
        }
    };

    // Parse account response (non-fatal if it fails)
    let plan_tier = match account_res {
        Ok(resp) if resp.status().is_success() => resp
            .json::<ClaudeAccountResponse>()
            .await
            .ok()
            .and_then(|acct| detect_claude_plan(&acct)),
        _ => None,
    };

    // Build windows from non-null entries
    let mut windows = Vec::new();
    let window_specs: &[(&str, &str, &Option<ClaudeWindowData>)] = &[
        ("five_hour", "Session (5hr)", &usage.five_hour),
        ("seven_day", "Weekly (7 day)", &usage.seven_day),
        ("seven_day_sonnet", "Weekly Sonnet", &usage.seven_day_sonnet),
        ("seven_day_opus", "Weekly Opus", &usage.seven_day_opus),
        (
            "seven_day_oauth_apps",
            "Weekly OAuth Apps",
            &usage.seven_day_oauth_apps,
        ),
        ("seven_day_cowork", "Weekly Cowork", &usage.seven_day_cowork),
        ("iguana_necktie", "Iguana Necktie", &usage.iguana_necktie),
    ];

    for (id, label, data) in window_specs {
        if let Some(w) = data {
            windows.push(RateLimitWindow::new(
                id.to_string(),
                label.to_string(),
                w.utilization,
                w.resets_at.clone(),
            ));
        }
    }

    let extra_usage = usage.extra_usage.map(normalize_claude_extra_usage);

    tracing::debug!(
        windows_count = windows.len(),
        plan_tier = ?plan_tier,
        has_extra_usage = extra_usage.is_some(),
        "Claude OAuth: API success"
    );

    FetchAttempt::Ok(ProviderRateLimits {
        provider: "claude".to_string(),
        plan_tier,
        windows,
        extra_usage,
        credits: None,
        stale: false,
        error: None,
        retry_after_seconds: None,
        cooldown_until: None,
        fetched_at: Local::now().to_rfc3339(),
    })
}

fn detect_claude_plan(acct: &ClaudeAccountResponse) -> Option<String> {
    for membership in &acct.memberships {
        if let Some(caps) = &membership.organization.capabilities {
            if caps.iter().any(|c| c == "claude_max") {
                // Use rate_limit_tier for more detail if available
                if let Some(tier) = &membership.organization.rate_limit_tier {
                    return Some(format_claude_plan_tier(tier));
                }
                return Some("Max".to_string());
            }
        }
    }
    // Fallback: check first membership with capabilities
    for membership in &acct.memberships {
        if let Some(caps) = &membership.organization.capabilities {
            if caps.contains(&"chat".to_string()) && !caps.contains(&"api".to_string()) {
                return Some("Pro".to_string());
            }
        }
    }
    None
}

fn format_claude_plan_tier(tier: &str) -> String {
    if tier.contains("claude_max_20x") {
        "Max 20x".to_string()
    } else if tier.contains("claude_max") {
        "Max 5x".to_string() // covers claude_max_5x and base Max plan ($100)
    } else if tier.contains("pro") {
        "Pro".to_string()
    } else {
        tier.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(target_os = "macos")]
    use std::sync::Mutex as StdMutex;
    use tempfile::TempDir;

    #[cfg(target_os = "macos")]
    static ENV_LOCK: StdMutex<()> = StdMutex::new(());

    fn credentials_json(token: &str) -> String {
        format!(
            r#"{{
  "claudeAiOauth": {{
    "accessToken": "{token}",
    "refreshToken": "refresh-token",
    "expiresAt": 1777084603000,
    "scopes": ["org:create_api_key"],
    "subscriptionType": "max",
    "rateLimitTier": "claude_max"
  }}
}}"#
        )
    }

    #[test]
    fn reads_access_token_from_credentials_file_payload() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join(".credentials.json");
        fs::write(&path, credentials_json("file-access-token")).unwrap();

        let token = read_token_from_credentials_path(&path).unwrap();

        assert_eq!(token, "file-access-token");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn oauth_token_prefers_credentials_file_on_macos() {
        let _guard = ENV_LOCK.lock().unwrap();
        let previous = std::env::var_os("CLAUDE_CONFIG_DIR");

        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join(".credentials.json"),
            credentials_json("macos-file-access-token"),
        )
        .unwrap();
        std::env::set_var("CLAUDE_CONFIG_DIR", tmp.path());

        let token = read_token_from_credentials_file().unwrap();

        assert_eq!(token, "macos-file-access-token");

        if let Some(value) = previous {
            std::env::set_var("CLAUDE_CONFIG_DIR", value);
        } else {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
    }

    #[tokio::test]
    #[ignore = "requires local Claude credentials and network access"]
    async fn live_fetches_full_claude_rate_limit_windows_from_credentials_file() {
        invalidate_access_token_cache();

        let rate_limits = fetch_claude_rate_limits().await.unwrap();
        let window_ids = rate_limits
            .windows
            .iter()
            .map(|window| window.window_id.as_str())
            .collect::<Vec<_>>();
        println!("Claude rate-limit windows: {window_ids:?}");

        assert!(window_ids.contains(&"five_hour"));
        assert!(window_ids.contains(&"seven_day"));
    }

    #[test]
    fn normalizes_claude_extra_usage_from_cents_to_usd() {
        let extra_usage = normalize_claude_extra_usage(ClaudeExtraUsageData {
            is_enabled: true,
            monthly_limit: 5000.0,
            used_credits: 710.0,
            utilization: Some(14.2),
        });

        assert!(extra_usage.is_enabled);
        assert_eq!(extra_usage.monthly_limit, 50.0);
        assert_eq!(extra_usage.used_credits, 7.1);
        assert_eq!(extra_usage.utilization, Some(14.2));
    }

    #[test]
    fn extract_access_token_from_valid_json() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-test","refreshToken":"rt"}}"#;
        assert_eq!(extract_access_token(json).unwrap(), "sk-ant-oat01-test");
    }

    #[test]
    fn extract_access_token_rejects_missing_field() {
        let json = r#"{"other": "data"}"#;
        assert!(extract_access_token(json).is_err());
    }

    #[test]
    fn extract_access_token_rejects_invalid_json() {
        assert!(extract_access_token("not json").is_err());
    }

    /// Serializes tests that touch the module-level token cache or the
    /// `CLAUDE_CODE_OAUTH_TOKEN` env var, both of which are global state.
    static SHARED_STATE_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn get_claude_oauth_token_reads_env_override() {
        let _guard = SHARED_STATE_LOCK.lock().unwrap();
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-from-env"}}"#;
        // SAFETY: serialized via SHARED_STATE_LOCK so no other test reads or
        // writes the same env var concurrently.
        unsafe {
            std::env::set_var("CLAUDE_CODE_OAUTH_TOKEN", json);
        }
        let result = get_claude_oauth_token();
        unsafe {
            std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
        }
        assert_eq!(result.unwrap(), "sk-from-env");
    }

    #[test]
    fn access_token_cache_stores_and_invalidates() {
        let _guard = SHARED_STATE_LOCK.lock().unwrap();
        invalidate_access_token_cache();
        assert!(cached_access_token().is_none());

        store_access_token("sk-cached");
        assert_eq!(cached_access_token().as_deref(), Some("sk-cached"));

        invalidate_access_token_cache();
        assert!(cached_access_token().is_none());
    }

    #[test]
    fn get_claude_oauth_token_returns_cached_value_without_keychain() {
        let _guard = SHARED_STATE_LOCK.lock().unwrap();
        // Make sure the env var is not set so we exercise the cache branch.
        // SAFETY: serialized via SHARED_STATE_LOCK.
        unsafe {
            std::env::remove_var("CLAUDE_CODE_OAUTH_TOKEN");
        }
        store_access_token("sk-from-cache");
        let result = get_claude_oauth_token();
        invalidate_access_token_cache();
        assert_eq!(result.unwrap(), "sk-from-cache");
    }
}
