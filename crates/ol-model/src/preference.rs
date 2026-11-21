#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LibrarySort {
    #[default]
    LastPlayed,
    Name,
}

impl LibrarySort {
    // A row of the settings table the Electron app neither reads nor returns: there the choice sits in the page's own storage.
    pub const KEY: &'static str = "library.sort";
    pub const ALL: [Self; 2] = [Self::LastPlayed, Self::Name];

    pub fn from_stored(stored: Option<&str>) -> Self {
        if stored == Some(Self::Name.as_str()) {
            Self::Name
        } else {
            Self::LastPlayed
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LastPlayed => "lastPlayed",
            Self::Name => "name",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::LastPlayed => "Last played",
            Self::Name => "Name",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sidebar {
    // The owner's default of 2026-11-21, at every window width.
    #[default]
    Folded,
    Open,
}

impl Sidebar {
    // The other row of `triage.toml` X-01, named and valued as the Electron app's page storage has it; that stored choice is not taken over.
    pub const KEY: &'static str = "sidebar";

    pub fn from_stored(stored: Option<&str>) -> Self {
        if stored == Some(Self::Open.as_str()) {
            Self::Open
        } else {
            Self::Folded
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Folded => "collapsed",
            Self::Open => "expanded",
        }
    }

    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SettingKey;

    #[test]
    fn anything_but_expanded_reads_as_a_folded_sidebar() {
        assert_eq!(Sidebar::from_stored(Some("expanded")), Sidebar::Open);
        for stored in [
            None,
            Some("collapsed"),
            Some("Expanded"),
            Some(""),
            Some("open"),
        ] {
            assert_eq!(Sidebar::from_stored(stored), Sidebar::Folded, "{stored:?}");
        }
        for sidebar in [Sidebar::Folded, Sidebar::Open] {
            assert_eq!(Sidebar::from_stored(Some(sidebar.as_str())), sidebar);
        }
        assert_eq!(Sidebar::default(), Sidebar::Folded);
        assert_eq!(SettingKey::parse(Sidebar::KEY), None);
    }

    #[test]
    fn anything_but_name_reads_as_last_played() {
        assert_eq!(LibrarySort::from_stored(Some("name")), LibrarySort::Name);
        for stored in [None, Some("lastPlayed"), Some("Name"), Some(""), Some("x")] {
            assert_eq!(
                LibrarySort::from_stored(stored),
                LibrarySort::LastPlayed,
                "{stored:?}"
            );
        }
        for sort in LibrarySort::ALL {
            assert_eq!(LibrarySort::from_stored(Some(sort.as_str())), sort);
        }
        assert_eq!(LibrarySort::default(), LibrarySort::LastPlayed);
    }

    #[test]
    fn the_key_is_not_one_of_the_electron_apps_settings() {
        assert_eq!(SettingKey::parse(LibrarySort::KEY), None);
    }
}
