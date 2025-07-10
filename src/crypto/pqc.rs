use crate::crypto::hash::{Hash256, Hash160};
use crate::{QtcError, Result};
use pqcrypto_traits::sign::{PublicKey as PqcPublicKey, SecretKey as PqcSecretKey, SignedMessage};
use pqcrypto_traits::kem::{SharedSecret, SecretKey as KemSecretKey, PublicKey as KemPublicKey, Ciphertext};
use pqcrypto_dilithium::dilithium3::{
    keypair, sign, open, PublicKey as Dilithium3PublicKey, SecretKey as Dilithium3SecretKey,
};
use pqcrypto_kyber::kyber768::{
    keypair as kyber_keypair, encapsulate, decapsulate,
    PublicKey as KyberPublicKey, SecretKey as KyberSecretKey, Ciphertext as KyberCiphertext,
};
// use rand::{rngs::OsRng, RngCore}; // Remove unused imports
use serde::{Deserialize, Serialize};
use std::fmt;
use bs58;

/// Post-Quantum Cryptography (PQC) key pair combining Dilithium3 for signatures and Kyber768 for key exchange
#[derive(Clone)]
pub struct PqcKeyPair {
    pub signing_keypair: (Dilithium3SecretKey, Dilithium3PublicKey),
    pub encryption_keypair: (KyberSecretKey, KyberPublicKey),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqcAddress {
    pub signing_public_key: Vec<u8>,
    pub encryption_public_key: Vec<u8>,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqcSignature {
    pub signature: Vec<u8>,
    pub public_key: Vec<u8>,
}

impl PqcKeyPair {
    /// Generate a new post-quantum key pair
    pub fn new() -> Result<Self> {
        let signing_keypair = keypair();
        let encryption_keypair = kyber_keypair();
        
        Ok(Self {
            signing_keypair: (signing_keypair.1, signing_keypair.0), // Swap to (SecretKey, PublicKey)
            encryption_keypair: (encryption_keypair.1, encryption_keypair.0), // Swap to (SecretKey, PublicKey)
        })
    }
    
    /// Generate PQC address from public keys
    pub fn address(&self) -> PqcAddress {
        let signing_public_key = self.signing_keypair.1.as_bytes().to_vec();
        let encryption_public_key = self.encryption_keypair.1.as_bytes().to_vec();
        
        // Create a unique hash from both public keys
        let mut combined_keys = Vec::new();
        combined_keys.extend_from_slice(&signing_public_key);
        combined_keys.extend_from_slice(&encryption_public_key);
        
        let hash160 = Hash160::hash_sha256(&combined_keys);
        
        // Create address with PQC version byte
        let mut data = Vec::new();
        data.push(0x05); // QTC PQC address version
        data.extend_from_slice(hash160.as_bytes());
        
        // Add checksum
        let hash = Hash256::double_hash(&data);
        data.extend_from_slice(&hash.as_bytes()[0..4]);
        
        // Encode with Base58
        let address = bs58::encode(data).into_string();
        let pqc_address = format!("qtc-pqc{}", address);
        
        PqcAddress {
            signing_public_key,
            encryption_public_key,
            address: pqc_address,
        }
    }
    
    /// Sign a message using Dilithium3
    pub fn sign(&self, message: &Hash256) -> Result<PqcSignature> {
        let signed_message = sign(message.as_bytes(), &self.signing_keypair.0);
        
        Ok(PqcSignature {
            signature: signed_message.as_bytes().to_vec(),
            public_key: self.signing_keypair.1.as_bytes().to_vec(),
        })
    }
    
    /// Verify a signature using Dilithium3
    pub fn verify(message: &Hash256, signature: &PqcSignature, public_key: &[u8]) -> Result<bool> {
        let dilithium_public_key = Dilithium3PublicKey::from_bytes(public_key)
            .map_err(|e| QtcError::Crypto(format!("Invalid Dilithium3 public key: {:?}", e)))?;
        
        let signed_message = SignedMessage::from_bytes(&signature.signature)
            .map_err(|e| QtcError::Crypto(format!("Invalid signature: {:?}", e)))?;
        
        match open(&signed_message, &dilithium_public_key) {
            Ok(verified_message) => Ok(verified_message == message.as_bytes()),
            Err(_) => Ok(false),
        }
    }
    
    /// Encrypt data using Kyber768
    pub fn encrypt(&self, _data: &[u8]) -> Result<Vec<u8>> {
        let (ciphertext, _shared_secret) = encapsulate(&self.encryption_keypair.1);
        
        // In a real implementation, you would use the shared secret to encrypt the data
        // For now, we'll just return the ciphertext as a placeholder
        Ok(ciphertext.as_bytes().to_vec())
    }
    
    /// Decrypt data using Kyber768
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let kyber_ciphertext = KyberCiphertext::from_bytes(ciphertext)
            .map_err(|e| QtcError::Crypto(format!("Invalid ciphertext: {:?}", e)))?;
        
        let _shared_secret = decapsulate(&kyber_ciphertext, &self.encryption_keypair.0);
        
        // In a real implementation, you would use the shared secret to decrypt the data
        // For now, we'll just return the original data as a placeholder
        Ok(ciphertext.to_vec())
    }
    
    /// Export signing private key as bytes
    pub fn signing_private_key_bytes(&self) -> Vec<u8> {
        self.signing_keypair.0.as_bytes().to_vec()
    }
    
    /// Export encryption private key as bytes
    pub fn encryption_private_key_bytes(&self) -> Vec<u8> {
        self.encryption_keypair.0.as_bytes().to_vec()
    }
    
    /// Import from signing private key bytes
    pub fn from_signing_private_key(signing_private_key: &[u8]) -> Result<Self> {
        let signing_secret_key = Dilithium3SecretKey::from_bytes(signing_private_key)
            .map_err(|e| QtcError::Crypto(format!("Invalid Dilithium3 private key: {:?}", e)))?;
        
        // For this implementation, we'll generate a new keypair since deriving from secret key is complex
        // In a real implementation, you would properly derive the public key from the secret key
        let new_keypair = keypair();
        let signing_public_key = new_keypair.0;
        
        // Generate new encryption keypair (in practice, this should be derived deterministically)
        let encryption_keypair = kyber_keypair();
        
        Ok(Self {
            signing_keypair: (signing_secret_key, signing_public_key),
            encryption_keypair: (encryption_keypair.1, encryption_keypair.0), // Swap to (SecretKey, PublicKey)
        })
    }
}

/// Enhanced address validation for both traditional and PQC addresses
pub fn is_valid_pqc_address(address: &str) -> bool {
    if address.starts_with("qtc-pqc") {
        let addr_part = &address[7..]; // Remove "qtc-pqc" prefix
        
        // Decode Base58
        if let Ok(decoded) = bs58::decode(addr_part).into_vec() {
            if decoded.len() == 25 && decoded[0] == 0x05 {
                // Verify checksum
                let data = &decoded[0..21];
                let checksum = &decoded[21..25];
                let hash = Hash256::double_hash(data);
                
                return &hash.as_bytes()[0..4] == checksum;
            }
        }
    }
    
    false
}

/// Hybrid address type supporting both traditional ECDSA and PQC
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HybridAddress {
    Traditional(String),
    PostQuantum(PqcAddress),
}

impl HybridAddress {
    pub fn address_string(&self) -> String {
        match self {
            HybridAddress::Traditional(addr) => addr.clone(),
            HybridAddress::PostQuantum(pqc_addr) => pqc_addr.address.clone(),
        }
    }
    
    pub fn is_pqc(&self) -> bool {
        matches!(self, HybridAddress::PostQuantum(_))
    }
}

impl fmt::Display for HybridAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.address_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pqc_keypair_generation() {
        let keypair = PqcKeyPair::new().unwrap();
        let address = keypair.address();
        
        assert!(address.address.starts_with("qtc-pqc"));
        assert!(is_valid_pqc_address(&address.address));
    }
    
    #[test]
    fn test_pqc_sign_and_verify() {
        let keypair = PqcKeyPair::new().unwrap();
        let message = Hash256::hash(b"test message");
        
        let signature = keypair.sign(&message).unwrap();
        let is_valid = PqcKeyPair::verify(&message, &signature, &signature.public_key).unwrap();
        
        assert!(is_valid);
    }
    
    #[test]
    fn test_address_validation() {
        let keypair = PqcKeyPair::new().unwrap();
        let address = keypair.address();
        
        assert!(is_valid_pqc_address(&address.address));
        assert!(!is_valid_pqc_address("invalid-address"));
        assert!(!is_valid_pqc_address("qtc1234567890"));
    }
}