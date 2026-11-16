#![forbid(unsafe_code)]

mod backend;

pub use backend::{ApiError, Backend, JobObserver, Library, StartLaunch};

pub mod library {
    pub use ol_services::library::{LibraryGame, caption, monogram, visible};
}

// What a launcher is reached through: the machine it runs on and the vault that keeps passwords.
pub mod launchers {
    pub use ol_adapters::steam::Timing as SteamTiming;
    pub use ol_services::accounts::NewAccount;
    pub use ol_services::launchers::{ImportResult, Launchers};
    pub use ol_vault::Secret;
}

pub mod doctor {
    pub use ol_services::doctor::{Report, run};

    // Only a build with the fake launchers can read the fixtures' stand-in vault: the app itself has no way to name it.
    #[cfg(feature = "fake")]
    pub fn run_on_fixtures(data_dir: &std::path::Path) -> Report {
        use ol_services::doctor::{VaultState, examine};
        examine(data_dir, &VaultState::StandIn(&ol_vault::FakeVault))
    }
}
