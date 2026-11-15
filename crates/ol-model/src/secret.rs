use std::fmt;

use crate::REDACTED;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecretKey {
    SteamGridDbApiKey,
    DiagnosticsRoundtrip,
    VaultProbe,
}

impl SecretKey {
    pub const ALL: [Self; 3] = [
        Self::SteamGridDbApiKey,
        Self::DiagnosticsRoundtrip,
        Self::VaultProbe,
    ];

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == text)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SteamGridDbApiKey => "steamgriddb.apiKey",
            Self::DiagnosticsRoundtrip => "diagnostics.roundtrip",
            Self::VaultProbe => "vault.probe",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Ciphertext(Vec<u8>);

impl Ciphertext {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

// Under the fixtures' stand-in vault a ciphertext is one XOR away from the secret, and its length is the secret's.
impl fmt::Debug for Ciphertext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ciphertext({REDACTED})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_reads_back() {
        for key in SecretKey::ALL {
            assert_eq!(SecretKey::parse(key.as_str()), Some(key));
        }
        assert_eq!(SecretKey::parse("vault.other"), None);
    }

    #[test]
    fn debug_shows_no_byte_and_no_length() {
        let shown = format!("{:?}", Ciphertext::new(vec![0x76, 0x31, 0x30, 0x41]));
        assert_eq!(shown, "Ciphertext([REDACTED])");
    }
}
