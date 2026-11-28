use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

pub const BRAND: &str = "icons/brand.svg";
pub const LIBRARY: &str = "icons/library.svg";
pub const ACCOUNTS: &str = "icons/accounts.svg";
pub const SETTINGS: &str = "icons/settings.svg";
pub const COLLAPSE: &str = "icons/collapse.svg";
pub const EXPAND: &str = "icons/expand.svg";
pub const SEARCH: &str = "icons/search.svg";
pub const FILTER: &str = "icons/filter.svg";
pub const SORT: &str = "icons/sort.svg";
pub const RESCAN: &str = "icons/rescan.svg";
pub const PLAY: &str = "icons/play.svg";
pub const GAMEPAD: &str = "icons/gamepad.svg";
pub const NO_MATCH: &str = "icons/no-match.svg";
pub const CLEAR: &str = "icons/clear.svg";
pub const UPDATE: &str = "icons/update.svg";
pub const CHECK: &str = "icons/check.svg";
pub const MORE: &str = "icons/more.svg";
pub const SWITCH: &str = "icons/switch.svg";
pub const DASHED: &str = "icons/dashed.svg";
pub const STOP: &str = "icons/stop.svg";
pub const HIDE: &str = "icons/hide.svg";
pub const FLASK: &str = "icons/flask.svg";
pub const INFO: &str = "icons/info.svg";
pub const KEY: &str = "icons/key.svg";
pub const SPINNER: &str = "icons/spinner.svg";
pub const POWER: &str = "icons/power.svg";
pub const RETRY: &str = "icons/retry.svg";
pub const FAILED: &str = "icons/failed.svg";
pub const CLOCK: &str = "icons/clock.svg";
pub const ARCHIVE: &str = "icons/archive.svg";
pub const FILE: &str = "icons/file.svg";
pub const HOURGLASS: &str = "icons/hourglass.svg";
pub const SLIDERS: &str = "icons/sliders.svg";
pub const CHEVRON_DOWN: &str = "icons/chevron-down.svg";
pub const CHEVRON_RIGHT: &str = "icons/chevron-right.svg";
pub const PLUS: &str = "icons/plus.svg";
pub const BACK: &str = "icons/back.svg";
pub const TRASH: &str = "icons/trash.svg";
pub const NOTE: &str = "icons/note.svg";
pub const EYE: &str = "icons/eye.svg";
pub const PERSON: &str = "icons/person.svg";
pub const MINIMIZE: &str = "icons/minimize.svg";
pub const MAXIMIZE: &str = "icons/maximize.svg";
pub const RESTORE: &str = "icons/restore.svg";
pub const CLOSE: &str = "icons/close.svg";
pub const DOWNLOAD: &str = "icons/download.svg";
pub const LIST_CHECKS: &str = "icons/list-checks.svg";
pub const COPY: &str = "icons/copy.svg";
pub const SAVE: &str = "icons/save.svg";
pub const SHIELD_CHECK: &str = "icons/shield-check.svg";
pub const SHIELD_OFF: &str = "icons/shield-off.svg";
pub const FOLDER_OPEN: &str = "icons/folder-open.svg";
pub const FOLDER_SEARCH: &str = "icons/folder-search.svg";
pub const ARROW_RIGHT: &str = "icons/arrow-right.svg";
pub const REPEAT: &str = "icons/repeat.svg";
pub const USER_PLUS: &str = "icons/user-plus.svg";
pub const EYE_PREVIEW: &str = "icons/eye-preview.svg";
pub const IMAGES: &str = "icons/images.svg";
pub const IMAGE_PLUS: &str = "icons/image-plus.svg";
pub const EXTERNAL_LINK: &str = "icons/external-link.svg";
pub const UPDATE_READY: &str = "icons/circle-arrow-down.svg";
pub const RESTART: &str = "icons/rotate-cw.svg";

macro_rules! oracle_icon {
    ($file:literal) => {
        include_bytes!(concat!("../../../oracle/assets/icons/", $file))
    };
}

macro_rules! caption_icon {
    ($shape:literal) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1">"#,
            $shape,
            "</svg>"
        )
        .as_bytes()
    };
}

const FILES: &[(&str, &[u8])] = &[
    (BRAND, oracle_icon!("build-icon-mono.svg")),
    (LIBRARY, oracle_icon!("lucide/LayoutGrid.svg")),
    (ACCOUNTS, oracle_icon!("lucide/Users.svg")),
    (SETTINGS, oracle_icon!("lucide/Settings.svg")),
    (COLLAPSE, oracle_icon!("lucide/PanelLeftClose.svg")),
    (EXPAND, oracle_icon!("lucide/PanelLeftOpen.svg")),
    (SEARCH, oracle_icon!("lucide/Search.svg")),
    (FILTER, oracle_icon!("lucide/Filter.svg")),
    (SORT, oracle_icon!("lucide/ArrowDownUp.svg")),
    (RESCAN, oracle_icon!("lucide/RefreshCw.svg")),
    (PLAY, oracle_icon!("lucide/Play.svg")),
    (GAMEPAD, oracle_icon!("lucide/Gamepad2.svg")),
    (NO_MATCH, oracle_icon!("lucide/SearchX.svg")),
    (CLEAR, oracle_icon!("lucide/X.svg")),
    (UPDATE, oracle_icon!("lucide/AlertTriangle.svg")),
    (CHECK, oracle_icon!("lucide/Check.svg")),
    (MORE, oracle_icon!("lucide/MoreHorizontal.svg")),
    (SWITCH, oracle_icon!("lucide/ArrowRightLeft.svg")),
    (DASHED, oracle_icon!("lucide/CircleDashed.svg")),
    (STOP, oracle_icon!("lucide/CircleStop.svg")),
    (HIDE, oracle_icon!("lucide/EyeOff.svg")),
    (FLASK, oracle_icon!("lucide/FlaskConical.svg")),
    (INFO, oracle_icon!("lucide/Info.svg")),
    (KEY, oracle_icon!("lucide/KeyRound.svg")),
    (SPINNER, oracle_icon!("lucide/Loader2.svg")),
    (POWER, oracle_icon!("lucide/Power.svg")),
    (RETRY, oracle_icon!("lucide/RotateCcw.svg")),
    (FAILED, oracle_icon!("lucide/XCircle.svg")),
    (CLOCK, oracle_icon!("lucide/Clock.svg")),
    (ARCHIVE, oracle_icon!("lucide/Archive.svg")),
    (FILE, oracle_icon!("lucide/FileText.svg")),
    (HOURGLASS, oracle_icon!("lucide/Hourglass.svg")),
    (SLIDERS, oracle_icon!("lucide/SlidersHorizontal.svg")),
    (CHEVRON_DOWN, oracle_icon!("lucide/ChevronDown.svg")),
    (CHEVRON_RIGHT, oracle_icon!("lucide/ChevronRight.svg")),
    (PLUS, oracle_icon!("lucide/Plus.svg")),
    (BACK, oracle_icon!("lucide/ArrowLeft.svg")),
    (TRASH, oracle_icon!("lucide/Trash2.svg")),
    (NOTE, oracle_icon!("lucide/StickyNote.svg")),
    (EYE, oracle_icon!("lucide/Eye.svg")),
    (PERSON, oracle_icon!("lucide/UserRound.svg")),
    (DOWNLOAD, oracle_icon!("lucide/Download.svg")),
    (LIST_CHECKS, oracle_icon!("lucide/ListChecks.svg")),
    (COPY, oracle_icon!("lucide/Copy.svg")),
    (SAVE, oracle_icon!("lucide/Save.svg")),
    (SHIELD_CHECK, oracle_icon!("lucide/ShieldCheck.svg")),
    (SHIELD_OFF, oracle_icon!("lucide/ShieldOff.svg")),
    (FOLDER_OPEN, oracle_icon!("lucide/FolderOpen.svg")),
    (FOLDER_SEARCH, oracle_icon!("lucide/FolderSearch.svg")),
    (ARROW_RIGHT, oracle_icon!("lucide/ArrowRight.svg")),
    (REPEAT, oracle_icon!("lucide/Repeat.svg")),
    (USER_PLUS, oracle_icon!("lucide/UserPlus.svg")),
    (EYE_PREVIEW, oracle_icon!("lucide/Eye.svg")),
    (IMAGES, oracle_icon!("lucide/Images.svg")),
    (IMAGE_PLUS, oracle_icon!("lucide/ImagePlus.svg")),
    (EXTERNAL_LINK, oracle_icon!("lucide/ExternalLink.svg")),
    (UPDATE_READY, oracle_icon!("lucide/CircleArrowDown.svg")),
    (RESTART, oracle_icon!("lucide/RotateCw.svg")),
    (MINIMIZE, caption_icon!(r#"<path d="M0 5.5h10"/>"#)),
    (
        MAXIMIZE,
        caption_icon!(r#"<rect x="0.5" y="0.5" width="9" height="9"/>"#),
    ),
    (
        RESTORE,
        caption_icon!(
            r#"<rect x="0.5" y="2.5" width="7" height="7"/><path d="M2.5 2.5v-2h7v7h-2"/>"#
        ),
    ),
    (
        CLOSE,
        caption_icon!(r#"<path d="M0.5 0.5l9 9M9.5 0.5l-9 9"/>"#),
    ),
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        Ok(FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}
