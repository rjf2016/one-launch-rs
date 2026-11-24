// A change of a setting as the Electron app's settings:set makes it: the rule of the key first, then the row, stored as the plain text the Electron app stores.

use ol_model::{SettingKey, SettingRefusal, check_setting};
use ol_store::{Store, StoreError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingError {
    #[error(transparent)]
    Refused(#[from] SettingRefusal),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl SettingError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Refused(refusal) => refusal.code(),
            Self::Store(error) => error.code(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Changed {
    pub key: SettingKey,
    // Automatic artwork was turned on from a stored `false`, which the Electron app answers by queueing every missing cover. Turned on from nothing stored, it queues nothing.
    pub turns_artwork_on: bool,
}

pub fn change(store: &Store, key: SettingKey, value: &str) -> Result<Changed, SettingError> {
    check_setting(key, value)?;
    let turns_artwork_on = key == SettingKey::ArtworkAutoFetch
        && value == "true"
        && store.setting(key)?.as_deref() == Some("false");
    store.set_setting(key, value)?;
    Ok(Changed {
        key,
        turns_artwork_on,
    })
}
