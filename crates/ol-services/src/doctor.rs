use std::fmt;
use std::path::Path;

use ol_model::{Ciphertext, LauncherId, SecretKey};
use ol_platform_api::DataProtector;
use ol_store::{SCHEMA_VERSION, Store, StoreError, database_path};
use ol_vault::{KeyError, OsCryptVault, PROBE_VALUE, Vault};

pub enum VaultState<'a> {
    Unwrapped(&'a dyn Vault),
    StandIn(&'a dyn Vault),
    NoKey,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Unwrapped,
    StandIn,
    Missing,
    Failed(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub found: usize,
    pub decrypted: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Probe {
    Absent,
    Good,
    Wrong,
    Unreadable,
    NotTried,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Database {
    pub schema_on_disk: u32,
    pub accounts: u64,
    pub accounts_by_launcher: [usize; 4],
    pub games: u64,
    pub games_installed: usize,
    pub settings: u64,
    pub settings_known: usize,
    pub secrets: u64,
    pub sign_ins: u64,
    pub passwords: Tally,
    pub sign_in_blobs: Tally,
    pub secret_values: Tally,
    pub probe: Probe,
}

// What the report says is counts, fixed words and the text of an error: never a name, a login, an id or a stored value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub database: Result<Database, String>,
    pub key: Key,
}

pub fn run(data_dir: &Path, protector: &dyn DataProtector) -> Report {
    match OsCryptVault::open(data_dir, protector) {
        Ok(vault) => examine(data_dir, &VaultState::Unwrapped(&vault)),
        Err(KeyError::Missing) => examine(data_dir, &VaultState::NoKey),
        Err(error) => examine(data_dir, &VaultState::Failed(error.to_string())),
    }
}

pub fn examine(data_dir: &Path, vault: &VaultState<'_>) -> Report {
    let (key, vault) = match vault {
        VaultState::Unwrapped(vault) => (Key::Unwrapped, Some(*vault)),
        VaultState::StandIn(vault) => (Key::StandIn, Some(*vault)),
        VaultState::NoKey => (Key::Missing, None),
        VaultState::Failed(reason) => (Key::Failed(reason.clone()), None),
    };
    Report {
        database: database(data_dir, vault).map_err(|error| error.to_string()),
        key,
    }
}

fn database(data_dir: &Path, vault: Option<&dyn Vault>) -> Result<Database, StoreError> {
    let opened = Store::open_read_only(&database_path(data_dir))?;
    let store = opened.store;
    let counts = store.row_counts()?;
    let accounts = store.accounts()?;
    let stored = store.ciphertexts()?;

    let opens = |value: &Ciphertext| vault.is_some_and(|vault| vault.decrypt(value).is_ok());
    let tally = |values: &[Ciphertext]| Tally {
        found: values.len(),
        decrypted: values.iter().filter(|value| opens(value)).count(),
    };
    let mut secret_values = Tally::default();
    let mut probe = Probe::Absent;
    for (key, value) in &stored.secrets {
        secret_values.found += 1;
        let opened = vault.map(|vault| vault.decrypt(value));
        if matches!(opened, Some(Ok(_))) {
            secret_values.decrypted += 1;
        }
        if key == SecretKey::VaultProbe.as_str() {
            probe = match opened {
                None => Probe::NotTried,
                Some(Ok(text)) if text.expose() == PROBE_VALUE => Probe::Good,
                Some(Ok(_)) => Probe::Wrong,
                Some(Err(_)) => Probe::Unreadable,
            };
        }
    }

    Ok(Database {
        schema_on_disk: opened.schema_on_disk,
        accounts: counts.accounts,
        accounts_by_launcher: LauncherId::ALL.map(|launcher| {
            accounts
                .iter()
                .filter(|account| account.launcher == launcher)
                .count()
        }),
        games: counts.games,
        games_installed: store.installed_games()?.len(),
        settings: counts.settings,
        settings_known: store.settings()?.len(),
        secrets: counts.secrets,
        sign_ins: counts.launcher_sessions,
        passwords: tally(&stored.passwords),
        sign_in_blobs: tally(&stored.sessions),
        secret_values,
        probe,
    })
}

impl Database {
    pub fn encrypted(&self) -> Tally {
        let all = [self.passwords, self.sign_in_blobs, self.secret_values];
        Tally {
            found: all.iter().map(|tally| tally.found).sum(),
            decrypted: all.iter().map(|tally| tally.decrypted).sum(),
        }
    }
}

impl Report {
    pub fn passed(&self) -> bool {
        let Ok(database) = &self.database else {
            return false;
        };
        let encrypted = database.encrypted();
        let key = match self.key {
            Key::Unwrapped | Key::StandIn => true,
            Key::Missing => encrypted.found == 0,
            Key::Failed(_) => false,
        };
        key && encrypted.decrypted == encrypted.found
            && matches!(database.probe, Probe::Absent | Probe::Good)
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let encrypted = self.database.as_ref().map(Database::encrypted).ok();
        match &self.database {
            Err(reason) => writeln!(f, "database: failed ({reason})")?,
            Ok(database) => database.fmt(f)?,
        }
        match &self.key {
            Key::Unwrapped => writeln!(f, "vault key: ok")?,
            Key::StandIn => writeln!(f, "vault key: none (the stand-in vault of the fixtures)")?,
            Key::Missing => match encrypted {
                Some(all) if all.found == 0 => {
                    writeln!(f, "vault key: none yet (nothing is encrypted)")?;
                }
                Some(_) => writeln!(f, "vault key: failed ({})", KeyError::Missing)?,
                None => writeln!(f, "vault key: none ({})", KeyError::Missing)?,
            },
            Key::Failed(reason) => writeln!(f, "vault key: failed ({reason})")?,
        }
        if let Ok(database) = &self.database {
            let all = database.encrypted();
            writeln!(
                f,
                "encrypted values: {} of {} decrypt (passwords {} of {}, sign-ins {} of {}, secrets {} of {})",
                all.decrypted,
                all.found,
                database.passwords.decrypted,
                database.passwords.found,
                database.sign_in_blobs.decrypted,
                database.sign_in_blobs.found,
                database.secret_values.decrypted,
                database.secret_values.found,
            )?;
            let probe = match database.probe {
                Probe::Absent => "none stored",
                Probe::Good => "ok",
                Probe::Wrong => "failed (it decrypts to another text)",
                Probe::Unreadable => "failed (it does not decrypt)",
                Probe::NotTried => "not tried (no vault key)",
            };
            writeln!(f, "vault probe: {probe}")?;
        }
        writeln!(f, "result: {}", if self.passed() { "ok" } else { "FAILED" })
    }
}

impl fmt::Display for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "database: ok")?;
        if self.schema_on_disk == SCHEMA_VERSION {
            writeln!(f, "schema version: {SCHEMA_VERSION}")?;
        } else {
            writeln!(
                f,
                "schema version: {} (read through a copy in memory, migrated to {SCHEMA_VERSION}; the file is unchanged)",
                self.schema_on_disk
            )?;
        }
        write!(f, "accounts: {} (", self.accounts)?;
        for (launcher, count) in LauncherId::ALL.iter().zip(self.accounts_by_launcher) {
            let comma = if *launcher == LauncherId::ALL[0] {
                ""
            } else {
                ", "
            };
            write!(f, "{comma}{} {count}", launcher.as_str())?;
        }
        let known: usize = self.accounts_by_launcher.iter().sum();
        let unknown = self.accounts.saturating_sub(known as u64);
        if unknown > 0 {
            write!(f, ", other launchers {unknown}")?;
        }
        writeln!(f, ")")?;
        writeln!(
            f,
            "games: {} ({} installed)",
            self.games, self.games_installed
        )?;
        let unknown = self.settings.saturating_sub(self.settings_known as u64);
        if unknown > 0 {
            writeln!(
                f,
                "settings: {} ({unknown} this version does not know)",
                self.settings
            )?;
        } else {
            writeln!(f, "settings: {}", self.settings)?;
        }
        writeln!(f, "secrets: {}", self.secrets)?;
        writeln!(f, "saved sign-ins: {}", self.sign_ins)
    }
}
