#![forbid(unsafe_code)]

// Fake launchers: a scenario folder laid down under a scratch folder, and a scripted Steam client that reacts to being started as the real one would. Nothing here reaches a real launcher, the real registry or the real process list.

mod scenario;
mod steam;

pub use scenario::{
    FakeLaunchers, ScenarioError, Seed, SeedAccount, lay_down, materialize, write_knobs,
};
pub use steam::{SteamKnobs, SteamSim};
