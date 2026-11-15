use std::fmt;

use zeroize::{Zeroize, Zeroizing};

use crate::REDACTED;

pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub(crate) fn from_utf8(mut bytes: Zeroizing<Vec<u8>>) -> Option<Self> {
        match String::from_utf8(std::mem::take(&mut *bytes)) {
            Ok(text) => Some(Self::new(text)),
            Err(not_text) => {
                not_text.into_bytes().zeroize();
                None
            }
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({REDACTED})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_shows_no_character_and_no_length() {
        let secret = Secret::new("placeholder-password-1".to_owned());
        assert_eq!(format!("{secret:?}"), "Secret([REDACTED])");
        assert_eq!(format!("{secret:#?}"), "Secret([REDACTED])");
        assert_eq!(format!("{:?}", Some(&secret)), "Some(Secret([REDACTED]))");
        assert_eq!(secret.expose(), "placeholder-password-1");
    }

    #[test]
    fn bytes_that_are_not_text_make_no_secret() {
        assert!(Secret::from_utf8(Zeroizing::new(vec![0xff, 0xfe])).is_none());
        let secret = Secret::from_utf8(Zeroizing::new("pässwörd".as_bytes().to_vec())).unwrap();
        assert_eq!(secret.expose(), "pässwörd");
    }
}
