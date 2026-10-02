//! Saved passwords live only in the operating system's credential store (Windows
//! Credential Manager, the Secret Service on Linux), keyed by profile id so a rename keeps
//! them. There is deliberately no file-based store: passwords are never written as
//! plaintext anywhere.

use anyhow::{Result, anyhow};
use zeroize::Zeroizing;

/// The credential store, behind a trait so password logic is tested without the real one.
pub trait SecretStore {
    /// `Ok` when the store can be used on this machine.
    fn status(&self) -> Result<()>;
    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>>;
    fn set(&self, id: &str, password: &str) -> Result<()>;
    /// `Ok(true)` when something was deleted.
    fn delete(&self, id: &str) -> Result<bool>;
}

/// Overrides the credential service name (used by the opt-in keyring test, so it never
/// touches real entries).
pub const SERVICE_ENV: &str = "LOPI_KEYRING_SERVICE";
const SERVICE: &str = "lopi";

/// The OS credential store.
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        let service = std::env::var(SERVICE_ENV)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| SERVICE.to_string());
        Self { service }
    }

    fn entry(&self, id: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, id).map_err(unavailable)
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn status(&self) -> Result<()> {
        match keyring::Entry::store_status() {
            Ok(()) => Ok(()),
            Err(err) => Err(unavailable_ref(err)),
        }
    }

    fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>> {
        match self.entry(id)?.get_password() {
            Ok(password) => Ok(Some(Zeroizing::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(anyhow!("cannot read the saved password: {err}")),
        }
    }

    fn set(&self, id: &str, password: &str) -> Result<()> {
        self.entry(id)?
            .set_password(password)
            .map_err(|err| anyhow!("cannot save the password: {err}"))
    }

    fn delete(&self, id: &str) -> Result<bool> {
        match self.entry(id)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(err) => Err(anyhow!("cannot delete the saved password: {err}")),
        }
    }
}

fn unavailable(err: keyring::Error) -> anyhow::Error {
    unavailable_ref(&err)
}

fn unavailable_ref(err: &keyring::Error) -> anyhow::Error {
    let hint = if cfg!(target_os = "linux") {
        " (on Linux this needs a running Secret Service such as GNOME Keyring or KWallet)"
    } else {
        ""
    };
    anyhow!("the system credential store is not available: {err}{hint}")
}

#[cfg(test)]
pub mod memory {
    //! An in-memory [`SecretStore`] for tests.

    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default)]
    pub struct MemoryStore {
        pub entries: RefCell<BTreeMap<String, String>>,
        /// Simulates a machine without a credential store.
        pub unavailable: bool,
    }

    impl MemoryStore {
        fn check(&self) -> Result<()> {
            if self.unavailable {
                anyhow::bail!("the system credential store is not available: test");
            }
            Ok(())
        }
    }

    impl SecretStore for MemoryStore {
        fn status(&self) -> Result<()> {
            self.check()
        }
        fn get(&self, id: &str) -> Result<Option<Zeroizing<String>>> {
            self.check()?;
            Ok(self.entries.borrow().get(id).cloned().map(Zeroizing::new))
        }
        fn set(&self, id: &str, password: &str) -> Result<()> {
            self.check()?;
            self.entries.borrow_mut().insert(id.into(), password.into());
            Ok(())
        }
        fn delete(&self, id: &str) -> Result<bool> {
            self.check()?;
            Ok(self.entries.borrow_mut().remove(id).is_some())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses the real OS credential store under a throw-away service name. Run by hand:
    /// `cargo test real_keyring -- --ignored`
    #[test]
    #[ignore]
    fn real_keyring_round_trip() {
        let service = format!("lopi-test-{}", fastrand::u64(..));
        let store = KeyringStore { service };
        store.status().unwrap();
        let id = "roundtrip";
        assert_eq!(store.get(id).unwrap(), None);
        store.set(id, "s3cret with spaces").unwrap();
        assert_eq!(
            store.get(id).unwrap().as_deref().map(String::as_str),
            Some("s3cret with spaces")
        );
        assert!(store.delete(id).unwrap());
        assert!(!store.delete(id).unwrap());
        assert_eq!(store.get(id).unwrap(), None);
    }
}
