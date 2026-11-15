use ol_platform_api::{DataProtector, ProtectError};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows::core::w;
use zeroize::{Zeroize, Zeroizing};

// For the signed-in user, without entropy and never with a prompt: what unwraps the os_crypt key Chromium wrapped.
#[derive(Clone, Copy, Debug, Default)]
pub struct Dpapi;

impl DataProtector for Dpapi {
    fn protect(&self, plain: &[u8]) -> Result<Vec<u8>, ProtectError> {
        let input = blob(plain).ok_or(ProtectError::Protect)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &raw const input,
                w!(""),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut output,
            )
        }
        .map_err(|_| ProtectError::Protect)?;
        let mut protected = take(output);
        Ok(std::mem::take(&mut *protected))
    }

    fn unprotect(&self, protected: &[u8]) -> Result<Zeroizing<Vec<u8>>, ProtectError> {
        let input = blob(protected).ok_or(ProtectError::Unprotect)?;
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &raw const input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &raw mut output,
            )
        }
        .map_err(|_| ProtectError::Unprotect)?;
        Ok(take(output))
    }
}

fn blob(bytes: &[u8]) -> Option<CRYPT_INTEGER_BLOB> {
    Some(CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(bytes.len()).ok()?,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

// After an unprotect these bytes are the key, so Windows' copy is wiped before it is handed back to be freed.
fn take(output: CRYPT_INTEGER_BLOB) -> Zeroizing<Vec<u8>> {
    if output.pbData.is_null() {
        return Zeroizing::new(Vec::new());
    }
    let bytes = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let copy = Zeroizing::new(bytes.to_vec());
    bytes.zeroize();
    unsafe { LocalFree(Some(HLOCAL(output.pbData.cast()))) };
    copy
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: [u8; 20] = [
        0x01, 0x00, 0x00, 0x00, 0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00,
        0xc0, 0x4f, 0xc2, 0x97, 0xeb,
    ];

    #[test]
    fn what_is_protected_opens_again_for_this_user() {
        let plain: Vec<u8> = (0..32).collect();
        let protected = Dpapi.protect(&plain).unwrap();
        assert!(protected.starts_with(&START), "not a DPAPI blob");
        assert!(protected.len() > plain.len());
        assert!(
            !protected.windows(plain.len()).any(|part| part == plain),
            "the plain bytes are in the blob"
        );
        assert_eq!(*Dpapi.unprotect(&protected).unwrap(), plain);
    }

    #[test]
    fn a_changed_or_made_up_blob_is_refused() {
        let protected = Dpapi.protect(&[7; 32]).unwrap();
        for index in [protected.len() - 1, protected.len() / 2] {
            let mut changed = protected.clone();
            changed[index] ^= 0x01;
            assert_eq!(
                Dpapi.unprotect(&changed).err(),
                Some(ProtectError::Unprotect),
                "byte {index}"
            );
        }
        for made_up in [&[][..], &[0; 4], &START, &[0x5a; 300]] {
            assert_eq!(
                Dpapi.unprotect(made_up).err(),
                Some(ProtectError::Unprotect)
            );
        }
        assert_eq!(
            Dpapi.unprotect(&protected[..protected.len() - 1]).err(),
            Some(ProtectError::Unprotect)
        );
    }
}
