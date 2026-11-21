#![forbid(unsafe_code)]

mod backend;

pub use backend::{
    ApiError, AppInfo, Backend, JobObserver, Library, SavedSignIn, SettingChange, StartLaunch,
};
pub use ol_services::capture::Captured as CapturedSignIn;

pub mod clipboard {
    pub use ol_platform_api::{Clipboard, ClipboardError};
    pub use ol_services::clipboard::{CLEAR_AFTER, CopyField, CopyResult, SleepingTimer, Timer};
}

// What the Settings screen needs beside the settings themselves, which are `ol_model::Settings`.
pub mod settings {
    pub use ol_services::data_folder::Folder;
    pub use ol_services::install::{AutostartStatus, UpdateStatus, UpdateStep, UpdateUnavailable};
    pub use ol_services::vault_health::{
        EncryptionState, KEY_READABLE, RESET_BANNER, RESET_WARNING, VaultBackend, VaultHealth,
        VaultReset, VaultResetRefusal, VaultResetSummary,
    };
    pub use ol_services::window::{
        AfterPlay, CloseAction, QuitQuestion, START_HIDDEN_FLAG, close_request_action,
        hide_after_instant_play, start_hidden,
    };
}

pub mod library {
    pub use ol_services::library::{
        HourClock, LibraryGame, caption, date_time, monogram, relative, time_of_day, visible,
    };
}

pub mod accounts {
    pub use ol_services::accounts::{AccountPatch, NewAccount};
    pub use ol_vault::Secret;
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
    pub use ol_adapters::battlenet::Timing as BattlenetTiming;
    pub use ol_adapters::epic::Timing as EpicTiming;
    pub use ol_adapters::riot::Timing as RiotTiming;
    pub use ol_adapters::steam::Timing as SteamTiming;
    pub use ol_services::accounts::NewAccount;
    pub use ol_services::launchers::{
        BattlenetMachine, EpicMachine, ImportResult, Launchers, RiotMachine,
    };
    pub use ol_vault::Secret;

    // Only a build with the fake launchers can name the fixtures' stand-in vault, which protects nothing.
    #[cfg(feature = "fake")]
    pub fn on_the_stand_in_vault(platform: ol_platform_api::Platform) -> Launchers {
        Launchers::on(platform, std::sync::Arc::new(ol_vault::FakeVault))
    }

    // The launchers on this PC. They get the data folder's own vault from `Backend::start_on_this_pc`, which opens its key, or makes one for a folder that has none, as the Electron app does at its first start.
    pub fn on_this_pc(platform: ol_platform_api::Platform) -> Launchers {
        Launchers::on(platform, std::sync::Arc::new(crate::backend::NoVault))
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
