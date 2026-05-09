//! Encryption layer for OpenVault.
//!
//! Provides AES-256-GCM encryption implementation with Argon2 key derivation.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use argon2::{Argon2, PasswordHasher};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::error::VaultError;

/// Result type for vault operations.
pub type CryptoResult<T> = Result<T, VaultError>;

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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EncryptionAlgorithm {
    /// AES-256-GCM (default, recommended for most use cases).
    Aes256Gcm,
}

impl Default for EncryptionAlgorithm {
    fn default() -> Self {
        EncryptionAlgorithm::Aes256Gcm
    }
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
        
        // Encode salt to base64 without padding
        let salt_b64 = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, salt);
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
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &self.0)
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

        // Generate random 12-byte nonce
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, data)
            .map_err(|e| VaultError::Crypto(format!("Encryption failed: {}", e)))?;

        // Prepend nonce to ciphertext
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
        
        // Encrypt in chunks of 64KB
        while self.buffer.len() >= 65536 {
            let chunk = self.buffer.drain(..65536).collect::<Vec<_>>();
            let encrypted = self.encryptor.encrypt(&chunk)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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
            // Read encrypted chunk (64KB + 12 bytes nonce)
            let mut chunk = vec![0u8; 65548];
            let n = self.reader.read(&mut chunk)?;
            
            if n == 0 {
                self.finished = true;
                return Ok(0);
            }
            
            chunk.truncate(n);
            let decrypted = self.decryptor.decrypt(&chunk)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
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
        // Ciphertext should be larger than plaintext (nonce + auth tag)
        assert!(ciphertext.len() > data.len());
    }

    #[test]
    fn test_aes_different_nonces() {
        let encryptor = AesGcmEncryption::generate();
        let data = b"Test data";

        let ciphertext1 = encryptor.encrypt(data).unwrap();
        let ciphertext2 = encryptor.encrypt(data).unwrap();

        // Same plaintext should produce different ciphertext (different nonces)
        assert_ne!(ciphertext1, ciphertext2);

        // But both should decrypt to the same plaintext
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
        // 1MB of data
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

        // Tamper with the ciphertext
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
        // Use different length salts to ensure different hashes
        let salt1 = b"shortsalt";
        let salt2 = b"longersaltvalue";
        
        let key1 = Key256::from_password("mypassword", salt1).unwrap();
        let key2 = Key256::from_password("mypassword", salt2).unwrap();
        
        // Different salts should produce different keys
        assert_ne!(key1.to_hex(), key2.to_hex());
        
        // Same password and salt should produce same key
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
}
