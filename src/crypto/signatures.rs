use crate::{QtcError, Result};
use secp256k1::{ecdsa::Signature as Secp256k1Signature, Secp256k1, Message, PublicKey as Secp256k1PublicKey, SecretKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    r: [u8; 32],
    s: [u8; 32],
    recovery_id: u8,
}

impl Signature {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 64 && bytes.len() != 65 {
            return Err(QtcError::Crypto("Invalid signature length".to_string()));
        }
        
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        
        r.copy_from_slice(&bytes[0..32]);
        s.copy_from_slice(&bytes[32..64]);
        
        let recovery_id = if bytes.len() == 65 { bytes[64] } else { 0 };
        
        Ok(Self { r, s, recovery_id })
    }
    
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(65);
        bytes.extend_from_slice(&self.r);
        bytes.extend_from_slice(&self.s);
        bytes.push(self.recovery_id);
        bytes
    }
    
    pub fn to_compact(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(64);
        bytes.extend_from_slice(&self.r);
        bytes.extend_from_slice(&self.s);
        bytes
    }
    
    pub fn from_secp256k1(signature: Secp256k1Signature) -> Self {
        // Convert signature to DER format for serialization
        let der_bytes = signature.serialize_der();
        
        Self {
            r: r.to_be_bytes(),
            s: s.to_be_bytes(),
            recovery_id: 0, // TODO: Implement recovery ID calculation
        }
    }
    
    pub fn to_secp256k1(&self) -> Result<Secp256k1Signature> {
        let r = secp256k1::scalar::Scalar::from_be_bytes(self.r)
            .map_err(|e| QtcError::Crypto(format!("Invalid r value: {:?}", e)))?;
        let s = secp256k1::scalar::Scalar::from_be_bytes(self.s)
            .map_err(|e| QtcError::Crypto(format!("Invalid s value: {:?}", e)))?;
        
        Ok(Secp256k1Signature::from_scalars(r, s)
            .map_err(|e| QtcError::Crypto(format!("Invalid signature: {}", e)))?)
    }
    
    pub fn recovery_id(&self) -> u8 {
        self.recovery_id
    }
    
    pub fn set_recovery_id(&mut self, recovery_id: u8) {
        self.recovery_id = recovery_id;
    }
    
    pub fn to_der(&self) -> Result<Vec<u8>> {
        let secp_sig = self.to_secp256k1()?;
        Ok(secp_sig.serialize_der().to_vec())
    }
    
    pub fn from_der(der: &[u8]) -> Result<Self> {
        let secp_sig = Secp256k1Signature::from_der(der)
            .map_err(|e| QtcError::Crypto(format!("Invalid DER signature: {}", e)))?;
        
        Ok(Self::from_secp256k1(secp_sig))
    }
}

// Signature creation and verification utilities
pub struct SignatureUtils;

impl SignatureUtils {
    pub fn sign(secret_key: &SecretKey, message_hash: &[u8; 32]) -> Result<Signature> {
        let secp = Secp256k1::new();
        let message = Message::from_slice(message_hash)
            .map_err(|e| QtcError::Crypto(format!("Invalid message: {}", e)))?;
        
        let signature = secp.sign_ecdsa(&message, secret_key);
        Ok(Signature::from_secp256k1(signature))
    }
    
    pub fn verify(public_key: &Secp256k1PublicKey, message_hash: &[u8; 32], signature: &Signature) -> Result<bool> {
        let secp = Secp256k1::new();
        let message = Message::from_slice(message_hash)
            .map_err(|e| QtcError::Crypto(format!("Invalid message: {}", e)))?;
        
        let secp_signature = signature.to_secp256k1()?;
        
        match secp.verify_ecdsa(&message, &secp_signature, public_key) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
    
    pub fn recover_public_key(message_hash: &[u8; 32], signature: &Signature) -> Result<Secp256k1PublicKey> {
        let secp = Secp256k1::new();
        let message = Message::from_slice(message_hash)
            .map_err(|e| QtcError::Crypto(format!("Invalid message: {}", e)))?;
        
        let recovery_id = secp256k1::ecdsa::RecoveryId::from_i32(signature.recovery_id as i32)
            .map_err(|e| QtcError::Crypto(format!("Invalid recovery ID: {}", e)))?;
        
        let recoverable_sig = secp256k1::ecdsa::RecoverableSignature::from_compact(
            &signature.to_compact(),
            recovery_id
        ).map_err(|e| QtcError::Crypto(format!("Invalid recoverable signature: {}", e)))?;
        
        secp.recover_ecdsa(&message, &recoverable_sig)
            .map_err(|e| QtcError::Crypto(format!("Failed to recover public key: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::{PrivateKey, PublicKey};
    use crate::crypto::hash::Hash256;
    
    #[test]
    fn test_signature_roundtrip() -> Result<()> {
        let private_key = PrivateKey::new()?;
        let public_key = private_key.public_key()?;
        let message = Hash256::hash(b"test message");
        
        let signature = private_key.sign(&message)?;
        let is_valid = public_key.verify(&message, &signature)?;
        
        assert!(is_valid);
        
        Ok(())
    }
    
    #[test]
    fn test_signature_serialization() -> Result<()> {
        let private_key = PrivateKey::new()?;
        let message = Hash256::hash(b"test message");
        let signature = private_key.sign(&message)?;
        
        let bytes = signature.to_bytes();
        let restored_signature = Signature::from_bytes(&bytes)?;
        
        assert_eq!(signature, restored_signature);
        
        Ok(())
    }
    
    #[test]
    fn test_der_encoding() -> Result<()> {
        let private_key = PrivateKey::new()?;
        let message = Hash256::hash(b"test message");
        let signature = private_key.sign(&message)?;
        
        let der = signature.to_der()?;
        let restored_signature = Signature::from_der(&der)?;
        
        // Note: DER encoding may not preserve recovery ID
        assert_eq!(signature.to_compact(), restored_signature.to_compact());
        
        Ok(())
    }
}
