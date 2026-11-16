#![forbid(unsafe_code)]

pub mod js;
pub mod vdf;
pub mod winpath;

pub mod epic {
    pub mod ini;
    pub mod launcher_log;
    pub mod manifests;
    pub mod session;
}

pub mod steam {
    pub mod bootstrap_log;
    pub mod login_log;
    pub mod loginusers;
    pub mod manifests;
}
