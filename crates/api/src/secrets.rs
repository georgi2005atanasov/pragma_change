//! Authenticated encryption; the key is a host secret, never a database value.
use crate::error::{Result, bad};
use aes_gcm::{
    Aes256Gcm, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;

fn key() -> Result<Vec<u8>> {
    let path = std::env::var("CREDENTIAL_KEY_FILE")
        .map_err(|_| bad("Configure CREDENTIAL_KEY_FILE before saving credentials"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&path)?.permissions().mode() & 0o077 != 0 {
            return Err(bad(
                "Credential key file must be readable only by its owner",
            ));
        }
    }
    let bytes = std::fs::read_to_string(path)?;
    let key = hex::decode(bytes.trim())
        .map_err(|_| bad("Credential key must contain 64 hexadecimal characters"))?;
    if key.len() != 32 {
        return Err(bad("Credential key must contain 64 hexadecimal characters"));
    }
    Ok(key)
}
fn encrypt(key: &[u8], scope: &str, text: &str) -> Result<String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| bad("Invalid encryption key"))?;
    let mut nonce = [0; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: text.as_bytes(),
                aad: scope.as_bytes(),
            },
        )
        .map_err(|_| bad("Credential encryption failed"))?;
    Ok(STANDARD.encode([nonce.to_vec(), ciphertext].concat()))
}
fn decrypt(key: &[u8], scope: &str, text: &str) -> Result<String> {
    let bytes = STANDARD
        .decode(text)
        .map_err(|_| bad("Invalid encrypted credential"))?;
    if bytes.len() < 28 {
        return Err(bad("Invalid encrypted credential"));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| bad("Invalid encryption key"))?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&bytes[..12]),
            Payload {
                msg: &bytes[12..],
                aad: scope.as_bytes(),
            },
        )
        .map_err(|_| {
            bad("Credential authentication failed; verify the configured credential key")
        })?;
    String::from_utf8(plain).map_err(|_| bad("Invalid credential encoding"))
}
pub(crate) fn seal(scope: &str, text: &str) -> Result<String> {
    encrypt(&key()?, scope, text)
}
pub(crate) fn open(scope: &str, text: &str) -> Result<String> {
    decrypt(&key()?, scope, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credentials_are_bound_to_resource_and_key() {
        let encrypted = encrypt(&[7; 32], "server-a", "PRIVATE KEY").unwrap();
        assert_eq!(
            decrypt(&[7; 32], "server-a", &encrypted).unwrap(),
            "PRIVATE KEY"
        );
        assert!(decrypt(&[7; 32], "server-b", &encrypted).is_err());
        assert!(decrypt(&[8; 32], "server-a", &encrypted).is_err());
        assert!(decrypt(&[7; 32], "server-a", "AA==").is_err());
        assert_ne!(
            encrypted,
            encrypt(&[7; 32], "server-a", "PRIVATE KEY").unwrap()
        );
    }
}
