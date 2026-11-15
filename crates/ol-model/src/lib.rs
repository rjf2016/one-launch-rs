#![forbid(unsafe_code)]

mod account;
mod game;
mod launcher;
mod secret;
mod settings;

pub use account::Account;
pub use game::{ArtworkSource, Game, GameArtwork, LastLaunch};
pub use launcher::LauncherId;
pub use secret::{Ciphertext, SecretKey};
pub use settings::{SettingKey, Settings};

const REDACTED: &str = "[REDACTED]";
