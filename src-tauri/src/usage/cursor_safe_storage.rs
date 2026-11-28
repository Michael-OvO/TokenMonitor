//! Decrypt Cursor IDE's at-rest access token.
//!
//! Since late 2026 Cursor no longer stores `cursorAuth/accessToken` as a bare
//! JWT. It wraps it with Electron's safeStorage (Chromium `os_crypt`):
//!
//! ```text
//! cursor-safe-storage:v1:<base64( "v10" || ciphertext… )>
//! ```
//!
//! The symmetric key lives in Cursor's `Local State` (`os_crypt.encrypted_key`),
//! itself OS-protected: DPAPI on Windows, a Keychain password on macOS, and
//! libsecret (or the `peanuts` default) on Linux. The cipher differs by OS too:
//! AES-256-GCM on Windows, AES-128-CBC elsewhere.
//!
//! On any failure we return `None`; the caller then falls back to a pasted
//! dashboard session token, exactly as when no credential is present.

const WRAPPED_PREFIX: &str = "cursor-safe-storage:";

/// Decrypt a stored Cursor token, or pass a bare (pre-encryption) token through.
///
/// Returns `None` only when a *wrapped* token cannot be decrypted; a plain JWT
/// from older Cursor builds is returned unchanged.
pub(crate) fn decrypt_cursor_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let Some(rest) = trimmed.strip_prefix(WRAPPED_PREFIX) else {
        return Some(trimmed.to_string()); // legacy bare JWT
    };
    // `rest` is `v1:<base64>` (maybe a future `vN:`); the ciphertext is the tail.
    let b64 = rest.rsplit(':').next()?;
    let blob = b64_decode(b64)?;
    let plain = os_crypt_decrypt(&blob)?;
    let token = String::from_utf8(plain).ok()?.trim().to_string();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

/// A Chromium `os_crypt` blob: a 3-byte version tag (`v10`/`v11`) then the
/// platform ciphertext.
fn os_crypt_decrypt(blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() < 3 {
        return None;
    }
    let (version, body) = blob.split_at(3);
    platform_decrypt(version, body)
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .ok()
}

/// `Local State`, honoring the same `CURSOR_USER_DIR` override as the state DB:
/// it sits beside the `User` directory, three levels above `state.vscdb`.
#[allow(dead_code)]
fn local_state_path() -> Option<std::path::PathBuf> {
    if let Some(db) = super::cursor_parser::cursor_global_state_path_from_env() {
        if let Some(root) = db
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
        {
            return Some(root.join("Local State"));
        }
    }
    crate::paths::cursor_local_state_default()
}

/// The AES key from `Local State`'s `os_crypt.encrypted_key`, OS-unwrapped.
#[allow(dead_code)]
fn os_crypt_key_bytes() -> Option<Vec<u8>> {
    let path = local_state_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let encoded = value.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    b64_decode(encoded)
}

// ── Windows: DPAPI-wrapped key + AES-256-GCM ────────────────────────────────

#[cfg(target_os = "windows")]
fn platform_decrypt(_version: &[u8], body: &[u8]) -> Option<Vec<u8>> {
    // body = nonce(12) || ciphertext || tag(16)
    if body.len() < 12 + 16 {
        return None;
    }
    let key = os_crypt_key()?;
    let (nonce, ciphertext) = body.split_at(12);
    gcm_decrypt(&key, nonce, ciphertext)
}

#[cfg(target_os = "windows")]
fn os_crypt_key() -> Option<Vec<u8>> {
    let bytes = os_crypt_key_bytes()?;
    let protected = bytes.strip_prefix(b"DPAPI")?;
    let key = dpapi_unprotect(protected)?;
    (key.len() == 32).then_some(key)
}

#[cfg(target_os = "windows")]
fn gcm_decrypt(key: &[u8], nonce: &[u8], ciphertext_and_tag: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext_and_tag)
        .ok()
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect(data: &[u8]) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    // CryptUnprotectData does not write through `pbData`; the cast is only to
    // satisfy the shared blob type.
    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(&input, None, None, None, None, 0, &mut output).ok()?;
        if output.pbData.is_null() {
            return None;
        }
        let key = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output.pbData as *mut core::ffi::c_void));
        Some(key)
    }
}

// ── macOS / Linux: PBKDF2-derived key + AES-128-CBC ─────────────────────────

#[cfg(not(target_os = "windows"))]
fn platform_decrypt(version: &[u8], body: &[u8]) -> Option<Vec<u8>> {
    let (password, iterations) = secret_password(version)?;
    let key = pbkdf2_sha1(&password, b"saltysalt", iterations, 16);
    cbc_decrypt(&key, body)
}

#[cfg(not(target_os = "windows"))]
fn cbc_decrypt(key: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return None;
    }
    // Chromium's CBC IV is a fixed 16 bytes of 0x20 (space); it is not stored.
    let iv = [0x20u8; 16];
    let decryptor = cbc::Decryptor::<aes::Aes128>::new_from_slices(key, &iv).ok()?;
    decryptor.decrypt_padded_vec_mut::<Pkcs7>(ciphertext).ok()
}

#[cfg(not(target_os = "windows"))]
fn pbkdf2_sha1(password: &[u8], salt: &[u8], rounds: u32, out_len: usize) -> Vec<u8> {
    let mut out = vec![0u8; out_len];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, salt, rounds, &mut out);
    out
}

#[cfg(target_os = "macos")]
fn secret_password(_version: &[u8]) -> Option<(Vec<u8>, u32)> {
    // Electron stores the os_crypt password under this Keychain generic item.
    // Its ACL trusts only Cursor, so a read with Keychain UI allowed pops a
    // modal password panel from the background refresh. Read with the UI
    // suppressed: it succeeds silently where the user already granted access
    // and otherwise fails, leaving the pasted-token fallback.
    let pw = crate::platform::macos::keychain::with_ui_suppressed(|| {
        security_framework::passwords::get_generic_password("Cursor Safe Storage", "Cursor")
    })
    .ok()?
    .ok()?;
    Some((pw, 1003))
}

// ponytail: Linux reads only the "peanuts" default (common on headless / no
// keyring). A v11 key held in libsecret under Chromium's own schema attributes
// is left to the pasted-token fallback; wire up a libsecret lookup if a real
// GNOME/KWallet user reports it.
#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn secret_password(_version: &[u8]) -> Option<(Vec<u8>, u32)> {
    Some((b"peanuts".to_vec(), 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_jwt_passes_through_untouched() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1c2VyIn0.sig";
        assert_eq!(decrypt_cursor_token(jwt).as_deref(), Some(jwt));
    }

    #[test]
    fn whitespace_only_is_none() {
        assert_eq!(decrypt_cursor_token("   \n"), None);
    }

    #[test]
    fn wrapped_but_unkeyed_token_fails_gracefully() {
        // Well-formed wrapper, random bytes: must not panic, just report None.
        let blob = b64_encode(b"v10nonce12345ciphertexttagtagtagtag");
        let value = format!("{WRAPPED_PREFIX}v1:{blob}");
        assert_eq!(decrypt_cursor_token(&value), None);
    }

    #[test]
    fn too_short_blob_is_none() {
        let value = format!("{WRAPPED_PREFIX}v1:{}", b64_encode(b"v1"));
        assert_eq!(decrypt_cursor_token(&value), None);
    }

    fn b64_encode(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn gcm_round_trips_with_a_known_key() {
        use aes_gcm::aead::{Aead, KeyInit, OsRng};
        use aes_gcm::{AeadCore, Aes256Gcm};

        let key = Aes256Gcm::generate_key(OsRng);
        let cipher = Aes256Gcm::new(&key);
        let nonce = Aes256Gcm::generate_nonce(OsRng);
        let plaintext = b"cursor-ide-access-token";
        let ct = cipher.encrypt(&nonce, plaintext.as_ref()).unwrap();

        let got = gcm_decrypt(key.as_slice(), nonce.as_slice(), &ct);
        assert_eq!(got.as_deref(), Some(plaintext.as_ref()));
    }
}
