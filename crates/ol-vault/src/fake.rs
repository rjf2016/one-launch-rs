use ol_model::Ciphertext;
use zeroize::Zeroizing;

use crate::{Secret, Vault, VaultError};

const MASK: u8 = 0x5a;

// The stand-in the fixtures under oracle/ were written with. It protects nothing: the app never stores a value through it.
#[derive(Clone, Copy, Debug, Default)]
pub struct FakeVault;

impl Vault for FakeVault {
    fn encrypt(&self, plaintext: &Secret) -> Result<Ciphertext, VaultError> {
        Ok(Ciphertext::new(
            plaintext.expose().bytes().map(|byte| byte ^ MASK).collect(),
        ))
    }

    fn decrypt(&self, ciphertext: &Ciphertext) -> Result<Secret, VaultError> {
        let plaintext = Zeroizing::new(
            ciphertext
                .as_bytes()
                .iter()
                .map(|byte| byte ^ MASK)
                .collect(),
        );
        Secret::from_utf8(plaintext).ok_or(VaultError::Unreadable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_each_byte_xor_5a() {
        let sealed = FakeVault
            .encrypt(&Secret::new("one-launch".to_owned()))
            .unwrap();
        assert_eq!(
            sealed.as_bytes(),
            [0x35, 0x34, 0x3f, 0x77, 0x36, 0x3b, 0x2f, 0x34, 0x39, 0x32]
        );
        assert!(FakeVault.decrypt(&sealed).unwrap().expose() == "one-launch");
        assert!(
            FakeVault
                .decrypt(&Ciphertext::new(Vec::new()))
                .unwrap()
                .expose()
                .is_empty()
        );
        assert_eq!(
            FakeVault.decrypt(&Ciphertext::new(vec![0xa5])).err(),
            Some(VaultError::Unreadable)
        );
    }
}
