#![forbid(unsafe_code)]

// Fake launchers: a scenario folder laid down under a scratch folder, and scripted Steam, Epic, Battle.net and Riot clients that react to being started as the real ones would. Nothing here reaches a real launcher, the real registry or the real process list.

mod battlenet;
mod epic;
mod riot;
mod scenario;
mod steam;

pub use battlenet::{BattlenetFolders, BattlenetKnobs, BattlenetScript, BattlenetSim};
pub use epic::{EpicKnobs, EpicSim};
pub use riot::{
    RiotFolders, RiotKnobs, RiotSim, fake_id_token, fake_session_file, session_block_lines,
};
pub use scenario::{
    FakeLaunchers, ScenarioError, Seed, SeedAccount, SeedSession, lay_down, materialize,
    materialize_in_real_time, write_knobs,
};
pub use steam::{SteamKnobs, SteamSim};
