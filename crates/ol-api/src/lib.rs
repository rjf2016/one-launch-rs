#![forbid(unsafe_code)]

mod backend;

pub use backend::{ApiError, Backend, JobObserver, Library, SavedSignIn, StartLaunch};

pub mod library {
    pub use ol_services::library::{LibraryGame, caption, monogram, relative, visible};
}

// What the launch dialog, its banner and the recovery dialog say about a switch.
pub mod launch {
    pub use ol_services::accounts::in_pick_order;
    pub use ol_services::phases::{
        Phase, PhaseId, PhaseNames, Recommend, RecoveryCopy, banner_text, failed_note, group_steps,
        phase_label, plan_phases, recovery_copy, sentence, stopped_note, update_note,
    };
}

// What a launcher is reached through: the machine it runs on and the vault that keeps passwords.
pub mod launchers {
    pub use ol_adapters::epic::Timing as EpicTiming;
    pub use ol_adapters::steam::Timing as SteamTiming;
    pub use ol_services::accounts::NewAccount;
    pub use ol_services::launchers::{EpicMachine, ImportResult, Launchers};
    pub use ol_vault::Secret;

    // Only a build with the fake launchers can name the fixtures' stand-in vault, which protects nothing.
    #[cfg(feature = "fake")]
    pub fn on_the_stand_in_vault(platform: ol_platform_api::Platform) -> Launchers {
        Launchers::on(platform, std::sync::Arc::new(ol_vault::FakeVault))
    }
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
