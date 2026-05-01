//! API token storage using the OS native keychain.
//!
//! - macOS  : Keychain Access
//! - Linux  : Secret Service / keyutils
//! - Windows: Credential Manager
//!
//! Tokens are never written to disk in plaintext by this module.

use keyring_core::{Entry, Error as KeyringError};

use crate::error::{CliError, CliResult};

const SERVICE: &str = "intentry-cli";
const ACCOUNT: &str = "default";

fn entry() -> CliResult<Entry> {
    // keyring v4 requires a default store to be initialised first.
    // Use the platform-native store.
    if keyring_core::get_default_store().is_none() {
        keyring::use_native_store(false)
            .map_err(|e| CliError::Generic(format!("keychain init error: {e}")))?;
    }
    Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| CliError::Generic(format!("keychain error: {e}")))
}

/// Persist an API token in the OS keychain.
pub fn store_token(token: &str) -> CliResult<()> {
    entry()?
        .set_password(token)
        .map_err(|e| CliError::Generic(format!("failed to store token in keychain: {e}")))
}

/// Retrieve the stored API token. Returns `None` when not logged in.
pub fn get_token() -> CliResult<Option<String>> {
    match entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(e) => Err(CliError::Generic(format!("failed to read token: {e}"))),
    }
}

/// Delete the stored API token (logout).
pub fn delete_token() -> CliResult<()> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()), // already logged out - not an error
        Err(e) => Err(CliError::Generic(format!("failed to delete token: {e}"))),
    }
}

/// Return the stored token or a user-friendly `Auth` error.
pub fn require_token() -> CliResult<String> {
    get_token()?.ok_or_else(|| CliError::Auth("not logged in - run `intr login` first".to_string()))
}
