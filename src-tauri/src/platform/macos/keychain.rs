//! macOS generic-password access that cannot raise a Keychain panel.
//!
//! # Why this exists
//!
//! TokenMonitor ships unsigned — `tauri.conf.json` declares no
//! `signingIdentity` — so every build is ad-hoc signed and its code identity
//! is derived from the binary's cdhash. That identity changes on every
//! rebuild *and* every auto-update.
//!
//! macOS records the *creating binary's* code identity in a legacy keychain
//! item's ACL. So any item TokenMonitor creates through Security.framework is
//! readable by exactly one build of TokenMonitor and prompt-gated for the
//! next. Worse, `security_framework`'s `set_generic_password` is a
//! find-then-modify when the item already exists — a decrypt authorization
//! plus a modify authorization — so an update against a mismatched ACL pops
//! the modal "enter your keychain password" panel. That fired from background
//! and startup code paths with no user gesture.
//!
//! Going through `/usr/bin/security` instead sidesteps it: the item's ACL
//! records *that* binary, which never changes, so reads and updates stay
//! silent across rebuilds and updates forever. This is exactly how Claude
//! Code stores its own OAuth credentials.
//!
//! # Tradeoff
//!
//! An item whose ACL trusts `/usr/bin/security` can be read by any process
//! running as the same user that invokes that binary, rather than by one
//! pinned application. On this app that is not a downgrade: the pinned-ACL
//! alternative does not survive a single rebuild, and when it fails the
//! caller's fallback is a plaintext file in the app data dir. A keychain item
//! at user-level protection is strictly better than that.

use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Run `f` with macOS Keychain user interaction disabled process-wide.
///
/// Two things make this worth centralising. `SecKeychainSetUserInteractionAllowed`
/// is a *process-global* flag, and `security_framework`'s RAII guard restores
/// it to **allowed** on drop rather than to its previous value — so two
/// overlapping guarded reads would let the first one's drop re-enable the UI
/// while the second is still running, unmasking exactly the panel the guard
/// exists to suppress. The mutex serialises them so the flag is only ever
/// owned by one caller at a time.
///
/// Use this for any in-process Security.framework read. Writes should not
/// exist at all — go through [`upsert_generic_password`] instead.
pub fn with_ui_suppressed<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    use security_framework::os::macos::keychain::SecKeychain;

    static SERIALIZE: Mutex<()> = Mutex::new(());

    // A poisoned lock only means some previous caller panicked mid-read; the
    // flag is still restored by then, so the guard is safe to reuse.
    let _serialize = SERIALIZE.lock().unwrap_or_else(|e| e.into_inner());
    let _ui = SecKeychain::disable_user_interaction()
        .map_err(|e| format!("Failed to disable Keychain UI: {e}"))?;
    Ok(f())
}

/// Upper bound on a single `security` invocation.
///
/// These calls answer instantly. The bound exists so that if an item ever
/// does end up ACL-gated against `/usr/bin/security`, the child blocks on a
/// modal panel rather than returning — and every caller here is synchronous,
/// some of them on the app's main thread during setup.
const TIMEOUT: Duration = Duration::from_secs(3);

/// Run `/usr/bin/security` with `args`, returning trimmed stdout.
fn run(args: &[&str]) -> Result<String, String> {
    let mut child = Command::new("/usr/bin/security")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run /usr/bin/security: {e}"))?;

    // Payloads are well under the pipe buffer, so the child always exits on
    // its own before we drain stdout — polling for exit first cannot deadlock.
    let deadline = Instant::now() + TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "security {} timed out (Keychain prompt?)",
                        args.first().copied().unwrap_or("")
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(e) => return Err(format!("Failed to wait for /usr/bin/security: {e}")),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to collect /usr/bin/security output: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "security exited with {}: {stderr}",
            output
                .status
                .code()
                .map_or_else(|| "signal".to_string(), |c| c.to_string())
        ));
    }

    Ok(String::from_utf8(output.stdout)
        .map_err(|e| format!("Invalid UTF-8 from /usr/bin/security: {e}"))?
        .trim()
        .to_string())
}

/// Read a generic password. `account` maps to `-a`; `None` matches on the
/// service alone.
pub fn find_generic_password(service: &str, account: Option<&str>) -> Result<String, String> {
    let mut args = vec!["find-generic-password"];
    if let Some(acct) = account {
        args.extend_from_slice(&["-a", acct]);
    }
    args.extend_from_slice(&["-s", service, "-w"]);

    let value = run(&args)?;
    if value.is_empty() {
        return Err("security returned an empty password".to_string());
    }
    Ok(value)
}

/// Create or update a generic password, then read it back to confirm the
/// write landed.
///
/// The payload goes through argv, briefly visible in `ps` to the same user.
/// `security` offers no stdin path for `-w`, and this is the same exposure
/// Claude Code accepts for its own credentials.
pub fn upsert_generic_password(service: &str, account: &str, value: &str) -> Result<(), String> {
    run(&[
        "add-generic-password",
        "-U",
        "-a",
        account,
        "-s",
        service,
        "-w",
        value,
    ])?;

    let stored = find_generic_password(service, Some(account))
        .map_err(|e| format!("Wrote item but could not read it back: {e}"))?;
    if stored != value {
        return Err("Keychain write did not take effect".to_string());
    }
    Ok(())
}

/// Delete a generic password. A missing item is reported as `Ok(false)`.
pub fn delete_generic_password(service: &str, account: &str) -> Result<bool, String> {
    match run(&["delete-generic-password", "-a", account, "-s", service]) {
        Ok(_) => Ok(true),
        Err(e) if e.contains("could not be found") || e.contains("SecKeychainSearchCopyNext") => {
            Ok(false)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A miss must surface as an ordinary `Err`, not a hang on a Keychain
    /// panel and not a panic — every caller here is synchronous.
    #[test]
    fn find_reports_missing_item_as_error() {
        let err = find_generic_password(
            "com.tokenmonitor.test.definitely-absent-service",
            Some("no-such-account"),
        )
        .unwrap_err();

        assert!(
            err.contains("security exited with"),
            "unexpected error shape: {err}"
        );
    }

    /// Deleting something that was never there is not an error.
    #[test]
    fn delete_is_idempotent_for_absent_items() {
        let existed = delete_generic_password(
            "com.tokenmonitor.test.definitely-absent-service",
            "no-such-account",
        )
        .unwrap();
        assert!(!existed);
    }

    /// Full round-trip against the real login keychain, under a service name
    /// no other code uses. Proves the CLI path is silent: if it were
    /// ACL-gated this would block on a panel and hit the 3s timeout.
    #[test]
    fn round_trip_create_update_read_delete() {
        const SERVICE: &str = "com.tokenmonitor.test.macos-keychain-roundtrip";
        const ACCOUNT: &str = "test-account";

        // Leave no residue if a previous run died mid-test.
        let _ = delete_generic_password(SERVICE, ACCOUNT);

        upsert_generic_password(SERVICE, ACCOUNT, "first-value").unwrap();
        assert_eq!(
            find_generic_password(SERVICE, Some(ACCOUNT)).unwrap(),
            "first-value"
        );

        // The update path is the one that used to prompt via Security.framework.
        upsert_generic_password(SERVICE, ACCOUNT, "second-value").unwrap();
        assert_eq!(
            find_generic_password(SERVICE, Some(ACCOUNT)).unwrap(),
            "second-value"
        );

        assert!(delete_generic_password(SERVICE, ACCOUNT).unwrap());
        assert!(find_generic_password(SERVICE, Some(ACCOUNT)).is_err());
    }
}
