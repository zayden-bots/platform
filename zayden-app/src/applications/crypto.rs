use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::{env, fmt};

use aws_lc_rs::aead::{AES_256_GCM, Aad, Nonce, RandomizedNonceKey};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use secrecy::{ExposeSecret, SecretString};

use super::ApplicationError;

const KEYS_VAR: &str = "CUSTOM_BOT_KEYS";
const ACTIVE_KEY_VAR: &str = "CUSTOM_BOT_ACTIVE_KEY";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedToken {
    pub key_id: i16,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

pub struct Keyring {
    active: i16,
    keys: HashMap<i16, RandomizedNonceKey>,
}

impl Debug for Keyring {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut ids = self.keys.keys().copied().collect::<Vec<_>>();
        ids.sort_unstable();
        f.debug_struct("Keyring")
            .field("active", &self.active)
            .field("key_ids", &ids)
            .finish()
    }
}

impl Keyring {
    pub fn from_env() -> Result<Self, ApplicationError> {
        let keys = env_var(KEYS_VAR)?;
        let active = env_var(ACTIVE_KEY_VAR)?;
        Self::parse(&keys, &active)
    }

    pub fn parse(keys: &str, active: &str) -> Result<Self, ApplicationError> {
        let mut parsed = HashMap::new();

        for entry in keys.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (id, encoded) = entry.split_once(':').ok_or_else(|| {
                ApplicationError::InvalidKeySpec("expected `id:base64`".to_owned())
            })?;
            let id = parse_key_id(id)?;
            let bytes = STANDARD
                .decode(encoded.trim())
                .map_err(|source| ApplicationError::KeyNotBase64 { id, source })?;
            let key = RandomizedNonceKey::new(&AES_256_GCM, &bytes)
                .map_err(|source| ApplicationError::KeyLength { id, source })?;

            if parsed.insert(id, key).is_some() {
                return Err(ApplicationError::InvalidKeySpec(format!(
                    "key {id} is listed twice"
                )));
            }
        }

        let active = parse_key_id(active)?;
        if !parsed.contains_key(&active) {
            return Err(ApplicationError::UnknownKey(active));
        }

        Ok(Self { active, keys: parsed })
    }

    #[must_use]
    pub const fn active_key_id(&self) -> i16 {
        self.active
    }

    pub fn seal(
        &self,
        application_id: i64,
        token: &SecretString,
    ) -> Result<SealedToken, ApplicationError> {
        self.seal_with(&application_id.to_be_bytes(), token)
    }

    pub fn open(
        &self,
        application_id: i64,
        sealed: &SealedToken,
    ) -> Result<SecretString, ApplicationError> {
        self.open_with(&application_id.to_be_bytes(), sealed)
    }

    pub fn seal_webhook(
        &self,
        webhook_id: i64,
        token: &SecretString,
    ) -> Result<SealedToken, ApplicationError> {
        self.seal_with(&webhook_aad(webhook_id), token)
    }

    pub fn open_webhook(
        &self,
        webhook_id: i64,
        sealed: &SealedToken,
    ) -> Result<SecretString, ApplicationError> {
        self.open_with(&webhook_aad(webhook_id), sealed)
    }

    fn seal_with(
        &self,
        aad: &[u8],
        token: &SecretString,
    ) -> Result<SealedToken, ApplicationError> {
        let key = self.key(self.active)?;
        let mut ciphertext = token.expose_secret().as_bytes().to_vec();

        let nonce = key
            .seal_in_place_append_tag(Aad::from(aad), &mut ciphertext)
            .map_err(ApplicationError::Encrypt)?;

        Ok(SealedToken {
            key_id: self.active,
            nonce: nonce.as_ref().to_vec(),
            ciphertext,
        })
    }

    fn open_with(
        &self,
        aad: &[u8],
        sealed: &SealedToken,
    ) -> Result<SecretString, ApplicationError> {
        let key = self.key(sealed.key_id)?;
        let nonce = Nonce::try_assume_unique_for_key(&sealed.nonce)
            .map_err(ApplicationError::MalformedNonce)?;
        let mut in_out = sealed.ciphertext.clone();

        let plaintext = key
            .open_in_place(nonce, Aad::from(aad), &mut in_out)
            .map_err(ApplicationError::Decrypt)?;

        Ok(SecretString::from(String::from_utf8(plaintext.to_vec())?))
    }

    fn key(&self, id: i16) -> Result<&RandomizedNonceKey, ApplicationError> {
        self.keys.get(&id).ok_or(ApplicationError::UnknownKey(id))
    }
}

fn webhook_aad(webhook_id: i64) -> [u8; 16] {
    let mut aad = [0; 16];
    aad[..8].copy_from_slice(b"webhook:");
    aad[8..].copy_from_slice(&webhook_id.to_be_bytes());
    aad
}

fn parse_key_id(id: &str) -> Result<i16, ApplicationError> {
    let id = id.trim();
    id.parse()
        .map_err(|source| ApplicationError::KeyId { id: id.to_owned(), source })
}

fn env_var(name: &'static str) -> Result<String, ApplicationError> {
    env::var(name).map_err(|source| ApplicationError::MissingEnvVar { name, source })
}
