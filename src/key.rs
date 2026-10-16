//! The key of the store (SQLCipher), and where it lives.
//!
//! A random secret kept in the OS keyring (Secret Service on Linux, Keychain on macOS,
//! Credential Manager on Windows). Without a reachable keyring (headless Linux, SSH), a
//! passphrase typed at startup, or given in `$GMESSAGES_TUI_PASSPHRASE`.

use std::error::Error;
use std::path::Path;

const KEYRING_SERVICE: &str = "gmessages-tui";
const PASSPHRASE_VAR: &str = "GMESSAGES_TUI_PASSPHRASE";

/// The key of the store at `path`: `$GMESSAGES_TUI_PASSPHRASE`, else its keyring entry (a
/// new random key if there is none and no store yet), else a passphrase asked on the
/// terminal.
pub fn store_key(path: &Path) -> Result<String, Box<dyn Error>> {
    if let Ok(passphrase) = std::env::var(PASSPHRASE_VAR) {
        return Ok(passphrase);
    }
    let exists = path.exists();
    let user = path.to_string_lossy();
    match keyring::Entry::new(KEYRING_SERVICE, &user) {
        Ok(entry) => match entry.get_password() {
            Ok(key) => return Ok(key),
            // A store without keyring entry was made with a passphrase.
            Err(keyring::Error::NoEntry) if exists => {}
            Err(keyring::Error::NoEntry) => {
                let key = random_key()?;
                entry.set_password(&key)?;
                return Ok(key);
            }
            Err(e) => eprintln!("Keyring unavailable ({e}), using a passphrase."),
        },
        Err(e) => eprintln!("Keyring unavailable ({e}), using a passphrase."),
    }
    ask_passphrase(exists)
}

/// Deletes the keyring entry of the store at `path`, if any. A keyring out of reach holds
/// nothing of ours: the store was made with a passphrase.
pub fn forget(path: &Path) -> Result<(), Box<dyn Error>> {
    let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, &path.to_string_lossy()) else {
        return Ok(());
    };
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) | Err(keyring::Error::NoStorageAccess(_)) => Ok(()),
        Err(e) => Err(format!("cannot delete the store key from the keyring: {e}").into()),
    }
}

fn random_key() -> Result<String, Box<dyn Error>> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn ask_passphrase(existing: bool) -> Result<String, Box<dyn Error>> {
    if existing {
        return Ok(rpassword::prompt_password("Store passphrase: ")?);
    }
    loop {
        let passphrase = rpassword::prompt_password("Choose a passphrase for the store: ")?;
        if passphrase.is_empty() {
            eprintln!("The passphrase cannot be empty.");
            continue;
        }
        if rpassword::prompt_password("Same passphrase again: ")? == passphrase {
            return Ok(passphrase);
        }
        eprintln!("The passphrases differ, try again.");
    }
}
