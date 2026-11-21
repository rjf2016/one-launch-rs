use std::sync::{Mutex, MutexGuard};

use ol_platform_api::{Clipboard, ClipboardError, Zeroizing};

use crate::OpLog;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    Private,
    Plain,
    ByTheUser,
}

#[derive(Default)]
struct State {
    held: Option<(Zeroizing<String>, Kept)>,
    refuse_private: bool,
    refuse_all: bool,
}

// The log never holds the text: a test asks `holds`.
pub struct RecordingClipboard {
    log: OpLog,
    state: Mutex<State>,
}

impl RecordingClipboard {
    pub fn new(log: OpLog) -> Self {
        Self {
            log,
            state: Mutex::new(State::default()),
        }
    }

    pub fn refusing_private_writes(log: OpLog) -> Self {
        let clipboard = Self::new(log);
        clipboard.state().refuse_private = true;
        clipboard
    }

    pub fn held_by_another_program(log: OpLog) -> Self {
        let clipboard = Self::new(log);
        clipboard.state().refuse_all = true;
        clipboard
    }

    pub fn user_copies(&self, text: &str) {
        self.state().held = Some((Zeroizing::new(text.to_owned()), Kept::ByTheUser));
    }

    pub fn holds(&self, text: &str) -> bool {
        self.state()
            .held
            .as_ref()
            .is_some_and(|(held, _)| held.as_str() == text)
    }

    pub fn kept(&self) -> Option<Kept> {
        self.state().held.as_ref().map(|(_, kept)| *kept)
    }

    pub fn is_empty(&self) -> bool {
        self.state().held.is_none()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self, text: &str, kept: Kept) -> Result<(), ClipboardError> {
        let mut state = self.state();
        if state.refuse_all || (kept == Kept::Private && state.refuse_private) {
            return Err(ClipboardError("the clipboard refused the write".to_owned()));
        }
        state.held = Some((Zeroizing::new(text.to_owned()), kept));
        drop(state);
        let private = kept == Kept::Private;
        self.log.record("clip.write", [("private", private.into())]);
        Ok(())
    }
}

impl Clipboard for RecordingClipboard {
    fn write_private(&self, text: &str) -> Result<(), ClipboardError> {
        self.write(text, Kept::Private)
    }

    fn write_text(&self, text: &str) -> Result<(), ClipboardError> {
        self.write(text, Kept::Plain)
    }

    fn read_text(&self) -> Result<Option<Zeroizing<String>>, ClipboardError> {
        let state = self.state();
        if state.refuse_all {
            return Err(ClipboardError("the clipboard refused the read".to_owned()));
        }
        Ok(state
            .held
            .as_ref()
            .map(|(held, _)| Zeroizing::new(held.as_str().to_owned())))
    }

    fn clear_if(&self, holds: &dyn Fn(&str) -> bool) -> Result<bool, ClipboardError> {
        let mut state = self.state();
        if state.refuse_all {
            return Err(ClipboardError("the clipboard refused the read".to_owned()));
        }
        if !state.held.as_ref().is_some_and(|(held, _)| holds(held)) {
            return Ok(false);
        }
        state.held = None;
        drop(state);
        self.log.record("clip.clear", []);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_is_written_down_without_its_text_and_a_clear_needs_the_same_text() {
        let log = OpLog::new();
        let clipboard = RecordingClipboard::new(log.clone());
        clipboard.write_private("placeholder-secret").unwrap();
        assert_eq!(clipboard.kept(), Some(Kept::Private));
        assert!(!clipboard.clear_if(&|held| held == "another").unwrap());
        assert!(clipboard.holds("placeholder-secret"));
        assert!(
            clipboard
                .clear_if(&|held| held == "placeholder-secret")
                .unwrap()
        );
        assert!(clipboard.is_empty());
        let events = serde_json::to_string(&log.snapshot()).unwrap();
        assert!(!events.contains("placeholder"));
        assert_eq!(
            log.snapshot()
                .iter()
                .map(|event| event["op"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>(),
            ["clip.write", "clip.clear"]
        );
    }

    #[test]
    fn a_clipboard_without_history_refuses_the_private_write_alone() {
        let clipboard = RecordingClipboard::refusing_private_writes(OpLog::new());
        assert!(clipboard.write_private("placeholder-secret").is_err());
        assert!(clipboard.is_empty());
        clipboard.write_text("placeholder-secret").unwrap();
        assert_eq!(clipboard.kept(), Some(Kept::Plain));
    }
}
