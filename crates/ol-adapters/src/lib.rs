#![forbid(unsafe_code)]

pub mod battlenet;
mod directory;
pub mod epic;
mod error;
pub mod files;
pub mod health;
mod launched;
pub mod riot;
mod running;
pub mod steam;

pub use directory::{AccountRef, Directory, GameRef};
pub use error::AdapterError;
pub use launched::Launched;
