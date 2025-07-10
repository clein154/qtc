use crate::crypto::hash::{Hash256, Hash160};
use crate::{QtcError, Result};
use secp256k1::{Secp256k1, SecretKey, PublicKey as Secp256k1PublicKey, Message, All};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone)]
pub struct PrivateKey {
    key: SecretKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKey {
    key: Vec<u8>, // Serialized public key
}

#[derive(Debug, Clone)]
pub struct KeyPair {
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
}

impl PrivateKey {
    pub fn new() -> Result<Self> {
        let mut rng = OsRng;
        let mut secret_bytes = [0u8; 32];
        rng.fill_bytes(&mut secret_bytes);
        
        let secret_key = SecretKey::from_slice(&secret_bytes)
            .map_err(|e| QtcError::Crypto(format!("Failed to create private key: {}", e)))?;
        
        Ok(Self { key: secret_key })
    }
    
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(QtcError::Crypto("Private key must be 32 bytes".to_string()));
        }
        
        let secret_key = SecretKey::from_slice(bytes)
            .map_err(|e| QtcError::Crypto(format!("Invalid private key: {}", e)))?;
        
        Ok(Self { key: secret_key })
    }
    
    pub fn from_wif(wif: &str) -> Result<Self> {
        // Decode WIF (Wallet Import Format)
        let decoded = bs58::decode(wif).into_vec()
            .map_err(|e| QtcError::Crypto(format!("Invalid WIF format: {}", e)))?;
        
        if decoded.len() != 37 || decoded[0] != 0x80 {
            return Err(QtcError::Crypto("Invalid WIF format".to_string()));
        }
        
        // Verify checksum
        let data = &decoded[0..33];
        let checksum = &decoded[33..37];
        let hash = Hash256::double_hash(data);
        
        if &hash.as_bytes()[0..4] != checksum {
            return Err(QtcError::Crypto("Invalid WIF checksum".to_string()));
        }
        
        Self::from_bytes(&decoded[1..33])
    }
    
    pub fn to_bytes(&self) -> [u8; 32] {
        self.key.secret_bytes()
    }
    
    pub fn to_wif(&self) -> String {
        let mut data = Vec::new();
        data.push(0x80); // QTC private key version
        data.extend_from_slice(&self.key.secret_bytes());
        
        // Add checksum
        let hash = Hash256::double_hash(&data);
        data.extend_from_slice(&hash.as_bytes()[0..4]);
        
        bs58::encode(data).into_string()
    }
    
    pub fn public_key(&self) -> Result<PublicKey> {
        let secp = Secp256k1::new();
        let public_key = Secp256k1PublicKey::from_secret_key(&secp, &self.key);
        
        Ok(PublicKey {
            key: public_key.serialize().to_vec(),
        })
    }
    
    pub fn sign(&self, message: &Hash256) -> Result<crate::crypto::signatures::Signature> {
        use crate::crypto::signatures::Signature;
        
        let secp = Secp256k1::new();
        let message = Message::from_slice(message.as_bytes())
            .map_err(|e| QtcError::Crypto(format!("Invalid message: {}", e)))?;
        
        let signature = secp.sign_ecdsa(&message, &self.key);
        Ok(Signature::from_secp256k1(signature))
    }
}

impl PublicKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 33 && bytes.len() != 65 {
            return Err(QtcError::Crypto("Invalid public key length".to_string()));
        }
        
        // Validate the public key
        let _ = Secp256k1PublicKey::from_slice(bytes)
            .map_err(|e| QtcError::Crypto(format!("Invalid public key: {}", e)))?;
        
        Ok(Self {
            key: bytes.to_vec(),
        })
    }
    
    pub fn to_bytes(&self) -> &[u8] {
        &self.key
    }
    
    pub fn hash160(&self) -> Hash160 {
        Hash160::hash_sha256(&self.key)
    }
    
    pub fn to_address(&self) -> String {
        let hash160 = self.hash160();
        
        // Create address with version byte
        let mut data = Vec::new();
        data.push(0x00); // QTC address version (P2PKH)
        data.extend_from_slice(hash160.as_bytes());
        
        // Add checksum
        let hash = Hash256::double_hash(&data);
        data.extend_from_slice(&hash.as_bytes()[0..4]);
        
        // Encode with Base58
        let address = bs58::encode(data).into_string();
        
        // Add QTC prefix
        format!("qtc{}", address)
    }
    
    pub fn verify(&self, message: &Hash256, signature: &crate::crypto::signatures::Signature) -> Result<bool> {
        let secp = Secp256k1::new();
        
        let public_key = Secp256k1PublicKey::from_slice(&self.key)
            .map_err(|e| QtcError::Crypto(format!("Invalid public key: {}", e)))?;
        
        let message = Message::from_slice(message.as_bytes())
            .map_err(|e| QtcError::Crypto(format!("Invalid message: {}", e)))?;
        
        let secp_signature = signature.to_secp256k1()?;
        
        match secp.verify_ecdsa(&message, &secp_signature, &public_key) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

impl KeyPair {
    pub fn new() -> Result<Self> {
        let private_key = PrivateKey::new()?;
        let public_key = private_key.public_key()?;
        
        Ok(Self {
            private_key,
            public_key,
        })
    }
    
    pub fn from_private_key(private_key: PrivateKey) -> Result<Self> {
        let public_key = private_key.public_key()?;
        
        Ok(Self {
            private_key,
            public_key,
        })
    }
    
    pub fn address(&self) -> String {
        self.public_key.to_address()
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.key))
    }
}

impl fmt::Display for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_wif())
    }
}

// Address utilities
pub fn address_to_hash160(address: &str) -> Result<Hash160> {
    if !address.starts_with("qtc") {
        return Err(QtcError::Crypto("Invalid QTC address prefix".to_string()));
    }
    
    let address_without_prefix = &address[3..];
    let decoded = bs58::decode(address_without_prefix).into_vec()
        .map_err(|e| QtcError::Crypto(format!("Invalid address format: {}", e)))?;
    
    if decoded.len() != 25 || decoded[0] != 0x00 {
        return Err(QtcError::Crypto("Invalid address format".to_string()));
    }
    
    // Verify checksum
    let data = &decoded[0..21];
    let checksum = &decoded[21..25];
    let hash = Hash256::double_hash(data);
    
    if &hash.as_bytes()[0..4] != checksum {
        return Err(QtcError::Crypto("Invalid address checksum".to_string()));
    }
    
    let mut hash160_bytes = [0u8; 20];
    hash160_bytes.copy_from_slice(&decoded[1..21]);
    
    Ok(Hash160::new(hash160_bytes))
}

pub fn is_valid_address(address: &str) -> bool {
    address_to_hash160(address).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_key_generation() -> Result<()> {
        let keypair = KeyPair::new()?;
        let address = keypair.address();
        
        assert!(address.starts_with("qtc"));
        assert!(is_valid_address(&address));
        
        Ok(())
    }
    
    #[test]
    fn test_wif_roundtrip() -> Result<()> {
        let private_key = PrivateKey::new()?;
        let wif = private_key.to_wif();
        let restored_key = PrivateKey::from_wif(&wif)?;
        
        assert_eq!(private_key.to_bytes(), restored_key.to_bytes());
        
        Ok(())
    }
    
    #[test]
    fn test_address_validation() -> Result<()> {
        let keypair = KeyPair::new()?;
        let address = keypair.address();
        
        assert!(is_valid_address(&address));
        assert!(!is_valid_address("invalid"));
        assert!(!is_valid_address("btc1qw508d6qejxtdg4y5r3zarvary0c5xw7kxdz6v9"));
        
        Ok(())
    }
}
