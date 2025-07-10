use sha2::{Sha256, Digest};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hash256([u8; 32]);

impl Hash256 {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    
    pub fn zero() -> Self {
        Self([0u8; 32])
    }
    
    pub fn hash(data: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(data);
        Self(hasher.finalize().into())
    }
    
    pub fn double_hash(data: &[u8]) -> Self {
        let first_hash = Self::hash(data);
        Self::hash(first_hash.as_bytes())
    }
    
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
    
    pub fn from_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex_str)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        Ok(Self(array))
    }
    
    pub fn from_slice(slice: &[u8]) -> Option<Self> {
        if slice.len() != 32 {
            return None;
        }
        
        let mut array = [0u8; 32];
        array.copy_from_slice(slice);
        Some(Self(array))
    }
}

impl fmt::Display for Hash256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl From<[u8; 32]> for Hash256 {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl AsRef<[u8]> for Hash256 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub trait Hashable {
    fn hash(&self) -> Hash256;
}

impl Hashable for &[u8] {
    fn hash(&self) -> Hash256 {
        Hash256::hash(self)
    }
}

impl Hashable for Vec<u8> {
    fn hash(&self) -> Hash256 {
        Hash256::hash(self)
    }
}

impl Hashable for String {
    fn hash(&self) -> Hash256 {
        Hash256::hash(self.as_bytes())
    }
}

// RIPEMD160 hash for address generation
use ripemd::{Ripemd160, Digest as RipemdDigest};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash160([u8; 20]);

impl Hash160 {
    pub fn new(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }
    
    pub fn hash(data: &[u8]) -> Self {
        let mut hasher = Ripemd160::new();
        hasher.update(data);
        Self(hasher.finalize().into())
    }
    
    pub fn hash_sha256(data: &[u8]) -> Self {
        let sha256_hash = Hash256::hash(data);
        Self::hash(sha256_hash.as_bytes())
    }
    
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
    
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for Hash160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hash256() {
        let data = b"hello world";
        let hash1 = Hash256::hash(data);
        let hash2 = Hash256::hash(data);
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, Hash256::zero());
    }
    
    #[test]
    fn test_hash256_hex() {
        let hash = Hash256::hash(b"test");
        let hex_str = hash.to_hex();
        let parsed_hash = Hash256::from_hex(&hex_str).unwrap();
        
        assert_eq!(hash, parsed_hash);
    }
    
    #[test]
    fn test_double_hash() {
        let data = b"test";
        let single = Hash256::hash(data);
        let double = Hash256::double_hash(data);
        
        assert_ne!(single, double);
        assert_eq!(double, Hash256::hash(single.as_bytes()));
    }
}
