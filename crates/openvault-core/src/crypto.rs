//! Encryption layer for OpenVault.
//!
//! Provides AES-256-GCM encryption implementation with Argon2 key derivation,
//! plus Phase 6 enhancements: VaultCrypto trait, PBKDF2 key derivation,
//! KeyManager for hierarchical key management, and EncryptedStorage decorator.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use argon2::{Argon2, PasswordHasher};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::VaultError;
use crate::snapshot::Snapshot;
use crate::storage::VaultStorage;

/// Result type for vault operations.
pub type CryptoResult<T> = Result<T, VaultError>;

// ============================================================================
// Phase 3: Core encryption (preserved)
// ============================================================================

/// Trait for encryption providers.
pub trait EncryptionProvider: Send + Sync {
    /// Encrypt data and return ciphertext with IV/nonce prepended.
    fn encrypt(&self, data: &[u8]) -> CryptoResult<Vec<u8>>;

    /// Decrypt data (IV/nonce must be stripped by caller).
    fn decrypt(&self, data: &[u8]) -> CryptoResult<Vec<u8>>;

    /// Get algorithm name.
    fn algorithm(&self) -> &str;
}

/// Supported encryption algorithms.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM (default, recommended for most use cases).
    #[default]
    Aes256Gcm,
}

/// 256-bit key storage.
#[derive(Clone)]
pub struct Key256([u8; 32]);

impl Key256 {
    /// Generate a new random key.
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    /// Create from existing bytes (caller must ensure 32 bytes).
    pub fn from_bytes(bytes: &[u8]) -> CryptoResult<Self> {
        if bytes.len() != 32 {
            return Err(VaultError::Crypto(format!(
                "Key must be exactly 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    /// Create from hex string.
    pub fn from_hex(hex: &str) -> CryptoResult<Self> {
        let bytes = hex::decode(hex)
            .map_err(|e| VaultError::Crypto(format!("Invalid hex key: {}", e)))?;
        Self::from_bytes(&bytes)
    }

    /// Create from password using Argon2 key derivation.
    pub fn from_password(password: &str, salt: &[u8]) -> CryptoResult<Self> {
        use argon2::password_hash::SaltString;
        // Ensure salt meets minimum length requirement (8 bytes for Argon2)
        let salt_vec: Vec<u8> = if salt.len() < 8 {
            let mut padded = salt.to_vec();
            padded.resize(8, 0);
            padded
        } else {
            salt.to_vec()
        };
        
        let salt_b64 = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &salt_vec);
        let salt = SaltString::from_b64(&salt_b64)
            .map_err(|e| VaultError::Crypto(format!("Invalid salt: {}", e)))?;
        
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| VaultError::Crypto(format!("Key derivation failed: {}", e)))?;
        
        let hash_bytes = hash.hash.ok_or_else(|| {
            VaultError::Crypto("Key derivation produced no hash".to_string())
        })?;
        
        let mut key = [0u8; 32];
        let hash_ref = hash_bytes.as_bytes();
        let len = std::cmp::min(32, hash_ref.len());
        key[..len].copy_from_slice(&hash_ref[..len]);
        Ok(Self(key))
    }

    /// Export as hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Export as base64 string.
    pub fn to_base64(&self) -> String {
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, self.0)
    }

    /// Get raw key bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Debug for Key256 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key256")
            .field("hex", &self.to_hex())
            .finish()
    }
}

/// AES-256-GCM encryption provider.
///
/// Each encryption call generates a random 12-byte nonce.
/// Nonce is prepended to ciphertext for decryption.
#[derive(Clone)]
pub struct AesGcmEncryption {
    key: Key256,
}

impl AesGcmEncryption {
    /// Create from key bytes.
    pub fn new(key: &[u8]) -> CryptoResult<Self> {
        Ok(Self {
            key: Key256::from_bytes(key)?,
        })
    }

    /// Create from hex string.
    pub fn from_hex(hex: &str) -> CryptoResult<Self> {
        Ok(Self {
            key: Key256::from_hex(hex)?,
        })
    }

    /// Create from base64 string.
    pub fn from_base64(b64: &str) -> CryptoResult<Self> {
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64)
            .map_err(|e| VaultError::Crypto(format!("Invalid base64 key: {}", e)))?;
        Ok(Self {
            key: Key256::from_bytes(&bytes)?,
        })
    }

    /// Generate a new encryption instance with random key.
    pub fn generate() -> Self {
        Self {
            key: Key256::generate(),
        }
    }

    /// Get the key for external storage (e.g., config).
    pub fn key(&self) -> Key256 {
        self.key.clone()
    }
}

impl EncryptionProvider for AesGcmEncryption {
    fn encrypt(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(&self.key.0)
            .map_err(|e| VaultError::Crypto(format!("Failed to create cipher: {}", e)))?;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, data)
            .map_err(|e| VaultError::Crypto(format!("Encryption failed: {}", e)))?;

        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    fn decrypt(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        if data.len() < 12 {
            return Err(VaultError::Crypto(
                "Ciphertext too short: must include 12-byte nonce".to_string(),
            ));
        }

        let cipher = Aes256Gcm::new_from_slice(&self.key.0)
            .map_err(|e| VaultError::Crypto(format!("Failed to create cipher: {}", e)))?;

        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| VaultError::Crypto(format!("Decryption failed: {}", e)))
    }

    fn algorithm(&self) -> &str {
        "AES-256-GCM"
    }
}

/// Factory for creating encryption providers.
pub struct EncryptionProviderFactory;

impl EncryptionProviderFactory {
    /// Create an encryption provider by algorithm.
    pub fn create(algorithm: EncryptionAlgorithm, key: &[u8]) -> CryptoResult<std::sync::Arc<dyn EncryptionProvider>> {
        match algorithm {
            EncryptionAlgorithm::Aes256Gcm => {
                Ok(std::sync::Arc::new(AesGcmEncryption::new(key)?) as std::sync::Arc<dyn EncryptionProvider>)
            }
        }
    }

    /// Create from hex-encoded key.
    pub fn create_from_hex(algorithm: EncryptionAlgorithm, hex_key: &str) -> CryptoResult<std::sync::Arc<dyn EncryptionProvider>> {
        match algorithm {
            EncryptionAlgorithm::Aes256Gcm => {
                Ok(std::sync::Arc::new(AesGcmEncryption::from_hex(hex_key)?) as std::sync::Arc<dyn EncryptionProvider>)
            }
        }
    }
}

/// Stream-based encryption writer for large files.
pub struct EncryptedWriter<W: std::io::Write> {
    writer: W,
    encryptor: AesGcmEncryption,
    buffer: Vec<u8>,
    processed: u64,
}

impl<W: std::io::Write> EncryptedWriter<W> {
    /// Create a new encrypted writer.
    pub fn new(writer: W, encryptor: AesGcmEncryption) -> Self {
        Self {
            writer,
            encryptor,
            buffer: Vec::new(),
            processed: 0,
        }
    }

    /// Finish encryption and write final data.
    pub fn finish(mut self) -> CryptoResult<(W, u64)> {
        if !self.buffer.is_empty() {
            let encrypted = self.encryptor.encrypt(&self.buffer)?;
            self.writer.write_all(&encrypted)?;
            self.processed += self.buffer.len() as u64;
        }
        Ok((self.writer, self.processed))
    }
}

impl<W: std::io::Write> std::io::Write for EncryptedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(buf);
        
        while self.buffer.len() >= 65536 {
            let chunk = self.buffer.drain(..65536).collect::<Vec<_>>();
            let encrypted = self.encryptor.encrypt(&chunk)
                .map_err(std::io::Error::other)?;
            self.writer.write_all(&encrypted)?;
            self.processed += chunk.len() as u64;
        }
        
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

/// Stream-based decryption reader for large files.
pub struct DecryptedReader<R: std::io::Read> {
    reader: R,
    decryptor: AesGcmEncryption,
    buffer: Vec<u8>,
    finished: bool,
}

impl<R: std::io::Read> DecryptedReader<R> {
    /// Create a new decrypted reader.
    pub fn new(reader: R, decryptor: AesGcmEncryption) -> Self {
        Self {
            reader,
            decryptor,
            buffer: Vec::new(),
            finished: false,
        }
    }
}

impl<R: std::io::Read> std::io::Read for DecryptedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.buffer.is_empty() && !self.finished {
            let mut chunk = vec![0u8; 65548];
            let n = self.reader.read(&mut chunk)?;
            
            if n == 0 {
                self.finished = true;
                return Ok(0);
            }
            
            chunk.truncate(n);
            let decrypted = self.decryptor.decrypt(&chunk)
                .map_err(std::io::Error::other)?;
            self.buffer = decrypted;
        }
        
        if self.buffer.is_empty() {
            return Ok(0);
        }
        
        let n = std::cmp::min(buf.len(), self.buffer.len());
        buf[..n].copy_from_slice(&self.buffer[..n]);
        self.buffer.drain(..n);
        Ok(n)
    }
}

// ============================================================================
// Phase 6: VaultCrypto trait & Aes256GcmCrypto
// ============================================================================

/// Encrypted data envelope with separated nonce for flexible storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    /// The encrypted ciphertext (without nonce).
    pub ciphertext: Vec<u8>,
    /// The nonce/IV used for encryption.
    pub nonce: Vec<u8>,
    /// Algorithm identifier.
    pub algorithm: EncryptionAlgorithm,
}

impl EncryptedData {
    /// Encode to a single byte buffer: [nonce_len(2 bytes)][nonce][ciphertext].
    pub fn to_bytes(&self) -> Vec<u8> {
        let nonce_len = self.nonce.len() as u16;
        let mut out = Vec::with_capacity(2 + self.nonce.len() + self.ciphertext.len());
        out.extend_from_slice(&nonce_len.to_be_bytes());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    /// Decode from a single byte buffer produced by `to_bytes`.
    pub fn from_bytes(data: &[u8]) -> CryptoResult<Self> {
        if data.len() < 2 {
            return Err(VaultError::Crypto("EncryptedData too short".to_string()));
        }
        let nonce_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        if data.len() < 2 + nonce_len {
            return Err(VaultError::Crypto("EncryptedData truncated nonce".to_string()));
        }
        let nonce = data[2..2 + nonce_len].to_vec();
        let ciphertext = data[2 + nonce_len..].to_vec();
        Ok(Self {
            ciphertext,
            nonce,
            algorithm: EncryptionAlgorithm::Aes256Gcm,
        })
    }
}

/// High-level cryptographic interface for OpenVault Phase 6.
///
/// Unlike `EncryptionProvider` which concatenates nonce+ciphertext,
/// `VaultCrypto` returns structured `EncryptedData` with separated fields,
/// enabling more flexible storage strategies.
pub trait VaultCrypto: Send + Sync {
    /// Encrypt data, returning structured encrypted envelope.
    fn encrypt(&self, data: &[u8]) -> CryptoResult<EncryptedData>;

    /// Decrypt structured encrypted data back to plaintext.
    fn decrypt(&self, data: &EncryptedData) -> CryptoResult<Vec<u8>>;

    /// Encrypt and serialize to flat byte buffer (convenience).
    fn encrypt_to_bytes(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        let enc = self.encrypt(data)?;
        Ok(enc.to_bytes())
    }

    /// Deserialize and decrypt from flat byte buffer (convenience).
    fn decrypt_from_bytes(&self, data: &[u8]) -> CryptoResult<Vec<u8>> {
        let enc = EncryptedData::from_bytes(data)?;
        self.decrypt(&enc)
    }

    /// Algorithm identifier.
    fn algorithm_name(&self) -> &str;
}

/// AES-256-GCM implementation of VaultCrypto.
///
/// Uses 12-byte random nonce per encryption. Provides authenticated encryption
/// (integrity + confidentiality) as required by modern security standards.
#[derive(Clone)]
pub struct Aes256GcmCrypto {
    key: Key256,
}

impl Aes256GcmCrypto {
    /// Create from raw key bytes (must be 32 bytes).
    pub fn new(key: &[u8]) -> CryptoResult<Self> {
        Ok(Self {
            key: Key256::from_bytes(key)?,
        })
    }

    /// Create from hex-encoded key.
    pub fn from_hex(hex: &str) -> CryptoResult<Self> {
        Ok(Self {
            key: Key256::from_hex(hex)?,
        })
    }

    /// Create from password using PBKDF2 key derivation.
    #[cfg(feature = "crypto-advanced")]
    pub fn from_password_pbkdf2(password: &str, salt: &[u8], iterations: u32) -> CryptoResult<Self> {
        let key = KeyDerivation::pbkdf2_derive(password, salt, iterations)?;
        Ok(Self { key })
    }

    /// Create from password using Argon2 key derivation.
    pub fn from_password_argon2(password: &str, salt: &[u8]) -> CryptoResult<Self> {
        let key = Key256::from_password(password, salt)?;
        Ok(Self { key })
    }

    /// Generate a new instance with a random key.
    pub fn generate() -> Self {
        Self {
            key: Key256::generate(),
        }
    }

    /// Access the underlying key.
    pub fn key(&self) -> &Key256 {
        &self.key
    }
}

impl VaultCrypto for Aes256GcmCrypto {
    fn encrypt(&self, data: &[u8]) -> CryptoResult<EncryptedData> {
        let cipher = Aes256Gcm::new_from_slice(self.key.as_bytes())
            .map_err(|e| VaultError::Crypto(format!("Failed to create cipher: {}", e)))?;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, data)
            .map_err(|e| VaultError::Crypto(format!("Encryption failed: {}", e)))?;

        Ok(EncryptedData {
            ciphertext,
            nonce: nonce_bytes.to_vec(),
            algorithm: EncryptionAlgorithm::Aes256Gcm,
        })
    }

    fn decrypt(&self, data: &EncryptedData) -> CryptoResult<Vec<u8>> {
        if data.nonce.len() != 12 {
            return Err(VaultError::Crypto(
                "Invalid nonce length: expected 12 bytes".to_string(),
            ));
        }

        let cipher = Aes256Gcm::new_from_slice(self.key.as_bytes())
            .map_err(|e| VaultError::Crypto(format!("Failed to create cipher: {}", e)))?;

        let nonce = Nonce::from_slice(&data.nonce);

        cipher
            .decrypt(nonce, data.ciphertext.as_slice())
            .map_err(|e| VaultError::Crypto(format!("Decryption failed: {}", e)))
    }

    fn algorithm_name(&self) -> &str {
        "AES-256-GCM"
    }
}

// ============================================================================
// Phase 6: KeyDerivation (PBKDF2)
// ============================================================================

/// PBKDF2-based key derivation for OpenVault.
///
/// Provides password-to-key derivation using PBKDF2-HMAC-SHA256,
/// suitable for scenarios where Argon2 is not available or where
/// compatibility with existing systems is required.
#[cfg(feature = "crypto-advanced")]
pub struct KeyDerivation;

#[cfg(feature = "crypto-advanced")]
impl KeyDerivation {
    /// Default PBKDF2 iteration count (600,000 rounds for SHA-256 per OWASP 2023).
    pub const DEFAULT_ITERATIONS: u32 = 600_000;

    /// Default salt length in bytes.
    pub const DEFAULT_SALT_LEN: usize = 32;

    /// Derive a 256-bit key from a password using PBKDF2-HMAC-SHA256.
    ///
    /// # Arguments
    /// * `password` - The password to derive from
    /// * `salt` - Cryptographic salt (should be unique per key)
    /// * `iterations` - Number of PBKDF2 iterations (higher = slower but more secure)
    pub fn pbkdf2_derive(password: &str, salt: &[u8], iterations: u32) -> CryptoResult<Key256> {
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(
            password.as_bytes(),
            salt,
            iterations,
            &mut key,
        );
        Key256::from_bytes(&key)
    }

    /// Derive a key with default iteration count.
    pub fn pbkdf2_derive_default(password: &str, salt: &[u8]) -> CryptoResult<Key256> {
        Self::pbkdf2_derive(password, salt, Self::DEFAULT_ITERATIONS)
    }

    /// Generate a random salt of the default length.
    pub fn generate_salt() -> Vec<u8> {
        let mut salt = vec![0u8; Self::DEFAULT_SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        salt
    }

    /// Derive a key from password with auto-generated salt.
    /// Returns (key, salt) so the salt can be stored for later derivation.
    pub fn derive_with_random_salt(password: &str) -> CryptoResult<(Key256, Vec<u8>)> {
        let salt = Self::generate_salt();
        let key = Self::pbkdf2_derive_default(password, &salt)?;
        Ok((key, salt))
    }
}

// ============================================================================
// Phase 6: KeyManager (hierarchical key management)
// ============================================================================

/// Metadata for a data key in the key hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataKeyInfo {
    /// Context/identifier for this data key.
    pub context: String,
    /// The derived key in hex.
    pub key_hex: String,
    /// Salt used for derivation.
    pub salt_hex: String,
    /// When this key was created.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Version of the master key that derived this data key.
    pub master_key_version: u32,
}

/// Hierarchical key manager for OpenVault.
///
/// Implements a two-tier key hierarchy:
/// - **Master key**: Top-level key derived from password or randomly generated.
///   Used only to derive data keys.
/// - **Data keys**: Per-context keys derived from the master key.
///   Used for actual data encryption.
///
/// This design enables key rotation: rotating the master key triggers
/// re-derivation of all data keys without re-encrypting data immediately.
/// Data can be re-encrypted lazily using the old data keys.
#[derive(Debug)]
pub struct KeyManager {
    /// Current master key.
    master_key: Key256,
    /// Version counter for master key rotations.
    master_key_version: u32,
    /// Derived data keys indexed by context.
    data_keys: HashMap<String, DataKeyInfo>,
    /// Old master keys retained for decrypting data encrypted with previous keys.
    old_master_keys: Vec<(u32, Key256)>,
}

impl KeyManager {
    /// Create a new KeyManager with the given master key.
    pub fn new(master_key: Key256) -> Self {
        Self {
            master_key,
            master_key_version: 1,
            data_keys: HashMap::new(),
            old_master_keys: Vec::new(),
        }
    }

    /// Create a KeyManager from a password using PBKDF2.
    #[cfg(feature = "crypto-advanced")]
    pub fn from_password(password: &str, salt: &[u8]) -> CryptoResult<Self> {
        let master_key = KeyDerivation::pbkdf2_derive_default(password, salt)?;
        Ok(Self::new(master_key))
    }

    /// Create a KeyManager from a password using Argon2.
    pub fn from_password_argon2(password: &str, salt: &[u8]) -> CryptoResult<Self> {
        let master_key = Key256::from_password(password, salt)?;
        Ok(Self::new(master_key))
    }

    /// Create a KeyManager with a randomly generated master key.
    pub fn generate() -> (Self, Key256) {
        let master_key = Key256::generate();
        let mgr = Self::new(master_key.clone());
        (mgr, master_key)
    }

    /// Derive a data key for a specific context.
    ///
    /// Uses HKDF-like construction: HMAC-SHA256(master_key, context || counter)
    /// to deterministically derive a context-specific key.
    /// If a data key already exists for this context, returns the existing one.
    pub fn derive_data_key(&mut self, context: &str) -> CryptoResult<Key256> {
        if let Some(info) = self.data_keys.get(context) {
            return Key256::from_hex(&info.key_hex);
        }

        // Derive using SHA-256(master_key || context || version) as a simple KDF
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.master_key.as_bytes());
        hasher.update(context.as_bytes());
        hasher.update(self.master_key_version.to_be_bytes());
        let hash = hasher.finalize();

        let data_key = Key256::from_bytes(&hash)?;

        // Generate a unique salt for this data key (for metadata purposes)
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);

        let info = DataKeyInfo {
            context: context.to_string(),
            key_hex: data_key.to_hex(),
            salt_hex: hex::encode(salt),
            created_at: chrono::Utc::now(),
            master_key_version: self.master_key_version,
        };
        self.data_keys.insert(context.to_string(), info);

        Ok(data_key)
    }

    /// Get a data key by context (without deriving a new one).
    pub fn get_data_key(&self, context: &str) -> Option<&DataKeyInfo> {
        self.data_keys.get(context)
    }

    /// List all data key contexts.
    pub fn list_contexts(&self) -> Vec<&str> {
        self.data_keys.keys().map(|s| s.as_str()).collect()
    }

    /// Rotate the master key.
    ///
    /// The old master key is retained so that data encrypted with data keys
    /// derived from the previous master key can still be decrypted.
    /// New data keys will be derived from the new master key.
    pub fn rotate_master_key(&mut self, new_master_key: Key256) -> CryptoResult<()> {
        // Retain old master key
        self.old_master_keys
            .push((self.master_key_version, self.master_key.clone()));

        // Install new master key
        self.master_key = new_master_key;
        self.master_key_version += 1;

        // Clear data keys so they will be re-derived on next access
        self.data_keys.clear();

        Ok(())
    }

    /// Rotate the master key by generating a new random one.
    /// Returns the new master key for secure storage.
    pub fn rotate_master_key_random(&mut self) -> Key256 {
        let new_key = Key256::generate();
        let key_clone = new_key.clone();
        let _ = self.rotate_master_key(new_key);
        key_clone
    }

    /// Get current master key version.
    pub fn master_key_version(&self) -> u32 {
        self.master_key_version
    }

    /// Get the number of data keys.
    pub fn data_key_count(&self) -> usize {
        self.data_keys.len()
    }

    /// Create a VaultCrypto instance for a specific context.
    pub fn create_crypto(&mut self, context: &str) -> CryptoResult<Aes256GcmCrypto> {
        let key = self.derive_data_key(context)?;
        Aes256GcmCrypto::new(key.as_bytes())
    }

    /// Get data key info for serialization/persistence.
    pub fn export_data_keys(&self) -> &HashMap<String, DataKeyInfo> {
        &self.data_keys
    }
}

// ============================================================================
// Phase 6: EncryptedStorage (transparent encryption decorator)
// ============================================================================

/// Transparent encryption decorator for VaultStorage.
///
/// Wraps any VaultStorage implementation and automatically encrypts/decrypts
/// file data on store/retrieve operations. Snapshot metadata is passed through
/// unencrypted to remain queryable.
pub struct EncryptedStorage {
    inner: Arc<dyn VaultStorage>,
    crypto: Arc<dyn VaultCrypto>,
}

impl EncryptedStorage {
    /// Create a new EncryptedStorage wrapping the given storage with the given crypto.
    pub fn new(storage: Arc<dyn VaultStorage>, crypto: Arc<dyn VaultCrypto>) -> Self {
        Self {
            inner: storage,
            crypto,
        }
    }

    /// Create with AES-256-GCM from a key.
    pub fn with_aes256gcm(storage: Arc<dyn VaultStorage>, key: &[u8]) -> CryptoResult<Self> {
        let crypto = Arc::new(Aes256GcmCrypto::new(key)?);
        Ok(Self::new(storage, crypto))
    }
}

impl VaultStorage for EncryptedStorage {
    fn store_file(&self, snapshot_id: &str, rel_path: &str, data: &[u8]) -> crate::error::VaultResult<()> {
        let encrypted = self.crypto.encrypt_to_bytes(data)
            .map_err(|e| VaultError::Storage(format!("Encryption failed for {}: {}", rel_path, e)))?;
        self.inner.store_file(snapshot_id, rel_path, &encrypted)
    }

    fn retrieve_file(&self, snapshot_id: &str, rel_path: &str) -> crate::error::VaultResult<Vec<u8>> {
        let encrypted = self.inner.retrieve_file(snapshot_id, rel_path)?;
        self.crypto.decrypt_from_bytes(&encrypted)
            .map_err(|e| VaultError::Storage(format!("Decryption failed for {}: {}", rel_path, e)))
    }

    fn store_snapshot(&self, snapshot: &Snapshot) -> crate::error::VaultResult<()> {
        // Pass through metadata unencrypted for queryability
        self.inner.store_snapshot(snapshot)
    }

    fn load_snapshot(&self, id: &str) -> crate::error::VaultResult<Snapshot> {
        self.inner.load_snapshot(id)
    }

    fn list_snapshots(&self) -> crate::error::VaultResult<Vec<Snapshot>> {
        self.inner.list_snapshots()
    }

    fn delete_snapshot(&self, id: &str) -> crate::error::VaultResult<()> {
        self.inner.delete_snapshot(id)
    }

    fn latest_snapshot(&self, source: String) -> crate::error::VaultResult<Option<Snapshot>> {
        self.inner.latest_snapshot(source)
    }

    fn latest_full_snapshot(&self, source: String) -> crate::error::VaultResult<Option<Snapshot>> {
        self.inner.latest_full_snapshot(source)
    }

    fn backend_name(&self) -> &str {
        "encrypted"
    }

    fn restore_snapshot(&self, snapshot: &Snapshot, target: &std::path::Path) -> crate::error::VaultResult<()> {
        // Custom restore that decrypts files
        std::fs::create_dir_all(target).map_err(|e| {
            VaultError::RestoreFailed(format!("Failed to create target directory: {}", e))
        })?;

        for entry in &snapshot.entries {
            let target_path = target.join(&entry.path);
            if let Some(parent) = target_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    VaultError::RestoreFailed(format!("Failed to create parent: {}", e))
                })?;
            }

            let data = self.retrieve_file(&snapshot.id, &entry.path)?;
            std::fs::write(&target_path, &data).map_err(|e| {
                VaultError::RestoreFailed(format!("Failed to write {}: {}", target_path.display(), e))
            })?;
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_encrypt_decrypt_roundtrip() {
        let key = Key256::generate();
        let encryptor = AesGcmEncryption::new(&key.0).unwrap();
        let data = b"Hello, OpenVault Phase 3!";

        let ciphertext = encryptor.encrypt(data).unwrap();
        let decrypted = encryptor.decrypt(&ciphertext).unwrap();

        assert_eq!(decrypted, data);
        assert!(ciphertext.len() > data.len());
    }

    #[test]
    fn test_aes_different_nonces() {
        let encryptor = AesGcmEncryption::generate();
        let data = b"Test data";

        let ciphertext1 = encryptor.encrypt(data).unwrap();
        let ciphertext2 = encryptor.encrypt(data).unwrap();

        assert_ne!(ciphertext1, ciphertext2);
        assert_eq!(encryptor.decrypt(&ciphertext1).unwrap(), data);
        assert_eq!(encryptor.decrypt(&ciphertext2).unwrap(), data);
    }

    #[test]
    fn test_empty_data_encryption() {
        let encryptor = AesGcmEncryption::generate();
        let empty = b"";

        let ciphertext = encryptor.encrypt(empty).unwrap();
        let decrypted = encryptor.decrypt(&ciphertext).unwrap();

        assert_eq!(decrypted, empty);
    }

    #[test]
    fn test_large_data_encryption() {
        let encryptor = AesGcmEncryption::generate();
        let large_data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

        let ciphertext = encryptor.encrypt(&large_data).unwrap();
        let decrypted = encryptor.decrypt(&ciphertext).unwrap();

        assert_eq!(decrypted, large_data);
    }

    #[test]
    fn test_wrong_key_decryption_fails() {
        let encryptor = AesGcmEncryption::generate();
        let wrong_key = Key256::generate();
        let wrong_decryptor = AesGcmEncryption::new(&wrong_key.0).unwrap();

        let ciphertext = encryptor.encrypt(b"Secret data").unwrap();
        let result = wrong_decryptor.decrypt(&ciphertext);

        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let encryptor = AesGcmEncryption::generate();
        let mut ciphertext = encryptor.encrypt(b"Original data").unwrap();

        if ciphertext.len() > 20 {
            ciphertext[20] ^= 0xFF;
        }

        let result = encryptor.decrypt(&ciphertext);
        assert!(result.is_err());
    }

    #[test]
    fn test_key_from_hex() {
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let key = Key256::from_hex(hex_key).unwrap();
        assert_eq!(key.to_hex(), hex_key);
    }

    #[test]
    fn test_key_from_hex_invalid() {
        let result = Key256::from_hex("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_key_from_password() {
        let salt1 = b"shortsalt";
        let salt2 = b"longersaltvalue";
        
        let key1 = Key256::from_password("mypassword", salt1).unwrap();
        let key2 = Key256::from_password("mypassword", salt2).unwrap();
        
        assert_ne!(key1.to_hex(), key2.to_hex());
        
        let key3 = Key256::from_password("mypassword", salt1).unwrap();
        assert_eq!(key1.to_hex(), key3.to_hex());
    }

    #[test]
    fn test_factory_create() {
        let key = Key256::generate();
        let provider = EncryptionProviderFactory::create(EncryptionAlgorithm::Aes256Gcm, &key.0).unwrap();
        
        let data = b"Test data";
        let ciphertext = provider.encrypt(data).unwrap();
        let decrypted = provider.decrypt(&ciphertext).unwrap();
        
        assert_eq!(decrypted, data);
    }

    // Phase 6 tests

    #[test]
    fn test_vault_crypto_roundtrip() {
        let crypto = Aes256GcmCrypto::generate();
        let data = b"Phase 6 encrypted data";

        let encrypted = crypto.encrypt(data).unwrap();
        assert_eq!(encrypted.nonce.len(), 12);
        assert_ne!(encrypted.ciphertext.as_slice(), data);

        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_vault_crypto_bytes_roundtrip() {
        let crypto = Aes256GcmCrypto::generate();
        let data = b"Flat buffer roundtrip";

        let bytes = crypto.encrypt_to_bytes(data).unwrap();
        let decrypted = crypto.decrypt_from_bytes(&bytes).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypted_data_serde() {
        let crypto = Aes256GcmCrypto::generate();
        let encrypted = crypto.encrypt(b"serde test").unwrap();

        let json = serde_json::to_string(&encrypted).unwrap();
        let decoded: EncryptedData = serde_json::from_str(&json).unwrap();

        let decrypted = crypto.decrypt(&decoded).unwrap();
        assert_eq!(decrypted, b"serde test");
    }

    #[test]
    fn test_encrypted_data_to_from_bytes() {
        let crypto = Aes256GcmCrypto::generate();
        let encrypted = crypto.encrypt(b"bytes test").unwrap();

        let bytes = encrypted.to_bytes();
        let restored = EncryptedData::from_bytes(&bytes).unwrap();

        let decrypted = crypto.decrypt(&restored).unwrap();
        assert_eq!(decrypted, b"bytes test");
    }

    #[test]
    fn test_vault_crypto_different_nonces() {
        let crypto = Aes256GcmCrypto::generate();
        let data = b"same data";

        let enc1 = crypto.encrypt(data).unwrap();
        let enc2 = crypto.encrypt(data).unwrap();

        // Different nonces
        assert_ne!(enc1.nonce, enc2.nonce);
        // Different ciphertext
        assert_ne!(enc1.ciphertext, enc2.ciphertext);
        // Both decrypt correctly
        assert_eq!(crypto.decrypt(&enc1).unwrap(), data);
        assert_eq!(crypto.decrypt(&enc2).unwrap(), data);
    }

    #[test]
    fn test_vault_crypto_algorithm_name() {
        let crypto = Aes256GcmCrypto::generate();
        assert_eq!(crypto.algorithm_name(), "AES-256-GCM");
    }

    #[test]
    fn test_vault_crypto_from_password_argon2() {
        let crypto = Aes256GcmCrypto::from_password_argon2("password123", b"salt123").unwrap();
        let data = b"argon2 derived key test";

        let encrypted = crypto.encrypt(data).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[cfg(feature = "crypto-advanced")]
    #[test]
    fn test_pbkdf2_key_derivation() {
        let key1 = KeyDerivation::pbkdf2_derive("password", b"salt", 1000).unwrap();
        let key2 = KeyDerivation::pbkdf2_derive("password", b"salt", 1000).unwrap();
        let key3 = KeyDerivation::pbkdf2_derive("password", b"differentsalt", 1000).unwrap();

        // Same inputs → same key
        assert_eq!(key1.to_hex(), key2.to_hex());
        // Different salt → different key
        assert_ne!(key1.to_hex(), key3.to_hex());
    }

    #[cfg(feature = "crypto-advanced")]
    #[test]
    fn test_pbkdf2_derive_with_random_salt() {
        let (key1, salt1) = KeyDerivation::derive_with_random_salt("password").unwrap();
        let (key2, salt2) = KeyDerivation::derive_with_random_salt("password").unwrap();

        // Different random salts
        assert_ne!(salt1, salt2);
        assert_ne!(key1.to_hex(), key2.to_hex());

        // Same salt → same key
        let key3 = KeyDerivation::pbkdf2_derive_default("password", &salt1).unwrap();
        assert_eq!(key1.to_hex(), key3.to_hex());
    }

    #[cfg(feature = "crypto-advanced")]
    #[test]
    fn test_vault_crypto_from_password_pbkdf2() {
        let crypto = Aes256GcmCrypto::from_password_pbkdf2("password", b"salt", 1000).unwrap();
        let data = b"pbkdf2 derived key test";

        let encrypted = crypto.encrypt(data).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_key_manager_basic() {
        let master_key = Key256::generate();
        let mut km = KeyManager::new(master_key);

        let key1 = km.derive_data_key("context1").unwrap();
        let key2 = km.derive_data_key("context2").unwrap();
        let key1_again = km.derive_data_key("context1").unwrap();

        // Different contexts → different keys
        assert_ne!(key1.to_hex(), key2.to_hex());
        // Same context → same key (cached)
        assert_eq!(key1.to_hex(), key1_again.to_hex());

        assert_eq!(km.data_key_count(), 2);
        assert_eq!(km.master_key_version(), 1);
    }

    #[test]
    fn test_key_manager_rotation() {
        let master_key = Key256::generate();
        let mut km = KeyManager::new(master_key);

        let key1_v1 = km.derive_data_key("context1").unwrap();
        assert_eq!(km.master_key_version(), 1);

        // Rotate master key
        let new_master = Key256::generate();
        km.rotate_master_key(new_master).unwrap();
        assert_eq!(km.master_key_version(), 2);

        // Data key for same context should be different now
        let key1_v2 = km.derive_data_key("context1").unwrap();
        assert_ne!(key1_v1.to_hex(), key1_v2.to_hex());
    }

    #[test]
    fn test_key_manager_create_crypto() {
        let master_key = Key256::generate();
        let mut km = KeyManager::new(master_key);

        let crypto = km.create_crypto("file-encryption").unwrap();
        let data = b"data encrypted via key manager";

        let encrypted = crypto.encrypt(data).unwrap();
        let decrypted = crypto.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_key_manager_generate() {
        let (km, master_key) = KeyManager::generate();
        assert_eq!(km.master_key_version(), 1);
        // Master key should be 32 bytes (64 hex chars)
        assert_eq!(master_key.to_hex().len(), 64);
    }

    #[test]
    fn test_key_manager_list_contexts() {
        let master_key = Key256::generate();
        let mut km = KeyManager::new(master_key);

        km.derive_data_key("docs").unwrap();
        km.derive_data_key("photos").unwrap();

        let mut contexts = km.list_contexts();
        contexts.sort();
        assert_eq!(contexts, vec!["docs", "photos"]);
    }
}
