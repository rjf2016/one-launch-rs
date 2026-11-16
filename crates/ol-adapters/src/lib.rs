#![forbid(unsafe_code)]

pub mod battlenet;
mod directory;
pub mod epic;
mod error;
pub mod files;
mod running;
pub mod steam;

pub use directory::{AccountRef, Directory};
pub use error::AdapterError;
