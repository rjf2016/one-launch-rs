use std::fmt;

use crate::LauncherId;

#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub launcher: LauncherId,
    pub display_name: String,
    pub login: String,
    pub has_secret: bool,
    pub has_session: bool,
    pub launcher_uid: Option<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub last_used_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// The name, the login, the launcher's own id and the notes say whose account this is: none of them is printed.
impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account")
            .field("launcher", &self.launcher)
            .field("has_secret", &self.has_secret)
            .field("has_session", &self.has_session)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_names_nobody() {
        let account = Account {
            id: "account-id".to_owned(),
            launcher: LauncherId::Epic,
            display_name: "Display Name".to_owned(),
            login: "login@example.com".to_owned(),
            has_secret: true,
            has_session: false,
            launcher_uid: Some("launcher-uid".to_owned()),
            tags: vec!["tag-one".to_owned()],
            notes: "some notes".to_owned(),
            last_used_at: None,
            created_at: "2026-01-02T03:04:05.678Z".to_owned(),
            updated_at: "2026-01-02T03:04:05.678Z".to_owned(),
        };
        let shown = format!("{account:?} {account:#?}");
        for hidden in [
            "account-id",
            "Display Name",
            "login@example.com",
            "launcher-uid",
            "tag-one",
            "some notes",
        ] {
            assert!(!shown.contains(hidden), "{hidden}");
        }
        assert!(shown.contains("Epic"));
    }
}
