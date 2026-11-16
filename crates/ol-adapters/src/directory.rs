// What an adapter may ask about One Launch's own accounts and their saved sign-ins. A saved sign-in comes out of it only as a `Secret`, and only for the switch pipeline to put into the launcher's own file.

use ol_model::LauncherSession;
use ol_vault::Secret;

use crate::AdapterError;

// The account a switch is for. It never carries the password.
#[derive(Clone, PartialEq, Eq)]
pub struct AccountRef {
    pub id: Option<String>,
    pub display_name: String,
    pub login: String,
    pub launcher_uid: Option<String>,
    pub has_secret: bool,
}

// The game a launch follows a switch with. Only Riot's client is started differently for one.
#[derive(Clone, Copy)]
pub struct GameRef<'a> {
    pub launcher_game_id: &'a str,
    pub name: &'a str,
}

// The accounts and saved sign-ins of one launcher.
pub trait Directory {
    fn account_by_launcher_uid(&self, uid: &str) -> Result<Option<AccountRef>, AdapterError>;
    fn account_by_id(&self, id: &str) -> Result<Option<AccountRef>, AdapterError>;
    // Records the launcher's own id on an account once a sign-in has shown it. Refused when another account holds the id.
    fn set_launcher_uid(&self, account_id: &str, uid: &str) -> Result<(), AdapterError>;
    fn session(&self, account_id: &str) -> Result<Option<LauncherSession>, AdapterError>;
    fn session_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Option<LauncherSession>, AdapterError>;
    // Encrypts the sign-in and stores it as the account's, in place of the one it had.
    fn save_session(
        &self,
        account_id: &str,
        launcher_uid: Option<&str>,
        fingerprint: &str,
        blob: &Secret,
    ) -> Result<LauncherSession, AdapterError>;
    fn decrypt_session(&self, account_id: &str) -> Result<Option<Secret>, AdapterError>;
    fn mark_session_verified(&self, account_id: &str) -> Result<(), AdapterError>;
    fn mark_session_stale(&self, account_id: &str, reason: &str) -> Result<(), AdapterError>;
}
