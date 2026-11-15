use aes_gcm::{AeadInOut, Aes256Gcm};
use zeroize::Zeroizing;

use crate::VaultError;

pub(crate) const PREFIX: &[u8; 3] = b"v10";
pub(crate) const NONCE_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const START: usize = PREFIX.len() + NONCE_BYTES;

pub(crate) fn seal(
    cipher: &Aes256Gcm,
    nonce: &[u8; NONCE_BYTES],
    plaintext: &[u8],
) -> Result<Vec<u8>, VaultError> {
    let mut blob = Zeroizing::new(Vec::with_capacity(START + plaintext.len() + TAG_BYTES));
    blob.extend_from_slice(PREFIX);
    blob.extend_from_slice(nonce);
    blob.extend_from_slice(plaintext);
    let tag = cipher
        .encrypt_inout_detached(nonce.into(), &[], (&mut blob[START..]).into())
        .map_err(|_| VaultError::Unavailable)?;
    blob.extend_from_slice(&tag);
    Ok(std::mem::take(&mut *blob))
}

// A value too short for the prefix, the nonce and the tag is damaged, the empty one included: the Electron app ends or reads an empty text there.
pub(crate) fn open(cipher: &Aes256Gcm, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    if blob.len() < START + TAG_BYTES || !blob.starts_with(PREFIX) {
        return Err(VaultError::Unreadable);
    }
    let (nonce, sealed) = blob[PREFIX.len()..].split_at(NONCE_BYTES);
    let (body, tag) = sealed.split_at(sealed.len() - TAG_BYTES);
    let nonce = nonce.try_into().map_err(|_| VaultError::Unreadable)?;
    let tag = tag.try_into().map_err(|_| VaultError::Unreadable)?;
    let mut plaintext = Zeroizing::new(body.to_vec());
    cipher
        .decrypt_inout_detached(nonce, &[], plaintext.as_mut_slice().into(), tag)
        .map_err(|_| VaultError::Unreadable)?;
    Ok(plaintext)
}
