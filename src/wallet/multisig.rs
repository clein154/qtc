use crate::core::Transaction;
use crate::crypto::keys::{PrivateKey, PublicKey};
use crate::crypto::signatures::Signature;
use crate::crypto::hash::Hash256;
use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultisigScript {
    pub required_signatures: u32,
    pub total_keys: u32,
    pub public_keys: Vec<PublicKey>,
    pub script: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultisigWallet {
    pub name: String,
    pub script: MultisigScript,
    pub our_key_indices: Vec<usize>, // Which keys we control
    pub address: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignature {
    pub signer_index: usize,
    pub signature: Signature,
    pub public_key: PublicKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureCollector {
    pub transaction: Transaction,
    pub input_index: usize,
    pub required_signatures: u32,
    pub signatures: HashMap<usize, PartialSignature>,
    pub script: MultisigScript,
}

impl MultisigScript {
    pub fn new(required: u32, public_keys: Vec<PublicKey>) -> Result<Self> {
        if required == 0 || required as usize > public_keys.len() {
            return Err(QtcError::Multisig("Invalid threshold".to_string()));
        }
        
        if public_keys.len() > 20 {
            return Err(QtcError::Multisig("Too many public keys (max 20)".to_string()));
        }
        
        let script = Self::create_multisig_script(required, &public_keys)?;
        
        Ok(Self {
            required_signatures: required,
            total_keys: public_keys.len() as u32,
            public_keys,
            script,
        })
    }
    
    fn create_multisig_script(required: u32, public_keys: &[PublicKey]) -> Result<Vec<u8>> {
        let mut script = Vec::new();
        
        // Push the required signature count
        script.push(0x50 + required as u8); // OP_1 through OP_16
        
        // Push all public keys
        for pubkey in public_keys {
            let pubkey_bytes = pubkey.to_bytes();
            script.push(pubkey_bytes.len() as u8);
            script.extend_from_slice(pubkey_bytes);
        }
        
        // Push the total number of public keys
        script.push(0x50 + public_keys.len() as u8);
        
        // OP_CHECKMULTISIG
        script.push(0xae);
        
        Ok(script)
    }
    
    pub fn to_address(&self) -> String {
        // Create P2SH address from script hash
        let script_hash = Hash256::hash(&self.script);
        
        let mut data = Vec::new();
        data.push(0x05); // P2SH address version
        data.extend_from_slice(&script_hash.as_bytes()[0..20]);
        
        // Add checksum
        let hash = Hash256::double_hash(&data);
        data.extend_from_slice(&hash.as_bytes()[0..4]);
        
        let address = bs58::encode(data).into_string();
        format!("qtc{}", address)
    }
    
    pub fn get_redeem_script(&self) -> &[u8] {
        &self.script
    }
    
    pub fn verify_signature_count(&self, signatures: &[Signature]) -> bool {
        signatures.len() >= self.required_signatures as usize
    }
}

impl MultisigWallet {
    pub fn new(
        name: String,
        required: u32,
        public_keys: Vec<PublicKey>,
        our_keys: Vec<usize>,
    ) -> Result<Self> {
        let script = MultisigScript::new(required, public_keys)?;
        let address = script.to_address();
        
        // Validate our_keys indices
        for &index in &our_keys {
            if index >= script.total_keys as usize {
                return Err(QtcError::Multisig("Invalid key index".to_string()));
            }
        }
        
        Ok(Self {
            name,
            script,
            our_key_indices: our_keys,
            address,
            created_at: chrono::Utc::now().timestamp() as u64,
        })
    }
    
    pub fn create_2_of_3(
        name: String,
        key1: PublicKey,
        key2: PublicKey,
        key3: PublicKey,
        our_key_index: usize,
    ) -> Result<Self> {
        let public_keys = vec![key1, key2, key3];
        Self::new(name, 2, public_keys, vec![our_key_index])
    }
    
    pub fn create_3_of_5(
        name: String,
        public_keys: Vec<PublicKey>,
        our_key_indices: Vec<usize>,
    ) -> Result<Self> {
        if public_keys.len() != 5 {
            return Err(QtcError::Multisig("Must provide exactly 5 public keys".to_string()));
        }
        
        Self::new(name, 3, public_keys, our_key_indices)
    }
    
    pub fn can_sign(&self) -> bool {
        !self.our_key_indices.is_empty()
    }
    
    pub fn required_signatures(&self) -> u32 {
        self.script.required_signatures
    }
    
    pub fn total_keys(&self) -> u32 {
        self.script.total_keys
    }
    
    pub fn get_public_keys(&self) -> &[PublicKey] {
        &self.script.public_keys
    }
    
    pub fn export_descriptor(&self) -> String {
        let pubkey_strings: Vec<String> = self.script.public_keys
            .iter()
            .map(|pk| hex::encode(pk.to_bytes()))
            .collect();
        
        format!(
            "multi({},{})",
            self.script.required_signatures,
            pubkey_strings.join(",")
        )
    }
    
    pub fn from_descriptor(name: String, descriptor: &str, our_indices: Vec<usize>) -> Result<Self> {
        // Parse miniscript descriptor
        // Simplified implementation - would use proper miniscript parsing in production
        
        if !descriptor.starts_with("multi(") || !descriptor.ends_with(")") {
            return Err(QtcError::Multisig("Invalid multisig descriptor".to_string()));
        }
        
        let inner = &descriptor[6..descriptor.len()-1];
        let parts: Vec<&str> = inner.split(',').collect();
        
        if parts.len() < 2 {
            return Err(QtcError::Multisig("Invalid descriptor format".to_string()));
        }
        
        let required: u32 = parts[0].parse()
            .map_err(|_| QtcError::Multisig("Invalid required signature count".to_string()))?;
        
        let mut public_keys = Vec::new();
        for pubkey_hex in &parts[1..] {
            let pubkey_bytes = hex::decode(pubkey_hex)
                .map_err(|_| QtcError::Multisig("Invalid public key hex".to_string()))?;
            let pubkey = PublicKey::from_bytes(&pubkey_bytes)?;
            public_keys.push(pubkey);
        }
        
        Self::new(name, required, public_keys, our_indices)
    }
}

impl SignatureCollector {
    pub fn new(
        transaction: Transaction,
        input_index: usize,
        script: MultisigScript,
    ) -> Self {
        Self {
            transaction,
            input_index,
            required_signatures: script.required_signatures,
            signatures: HashMap::new(),
            script,
        }
    }
    
    pub fn add_signature(&mut self, signer_index: usize, signature: Signature) -> Result<()> {
        if signer_index >= self.script.total_keys as usize {
            return Err(QtcError::Multisig("Invalid signer index".to_string()));
        }
        
        let public_key = self.script.public_keys[signer_index].clone();
        
        // Verify the signature
        let signature_hash = self.transaction.get_signature_hash(self.input_index);
        if !public_key.verify(&signature_hash, &signature)? {
            return Err(QtcError::Multisig("Invalid signature".to_string()));
        }
        
        let partial_sig = PartialSignature {
            signer_index,
            signature,
            public_key,
        };
        
        self.signatures.insert(signer_index, partial_sig);
        
        Ok(())
    }
    
    pub fn sign_with_key(&mut self, signer_index: usize, private_key: &PrivateKey) -> Result<()> {
        if signer_index >= self.script.total_keys as usize {
            return Err(QtcError::Multisig("Invalid signer index".to_string()));
        }
        
        // Verify the private key matches the expected public key
        let expected_pubkey = &self.script.public_keys[signer_index];
        let actual_pubkey = private_key.public_key()?;
        
        if expected_pubkey.to_bytes() != actual_pubkey.to_bytes() {
            return Err(QtcError::Multisig("Private key doesn't match public key".to_string()));
        }
        
        let signature_hash = self.transaction.get_signature_hash(self.input_index);
        let signature = private_key.sign(&signature_hash)?;
        
        self.add_signature(signer_index, signature)
    }
    
    pub fn is_complete(&self) -> bool {
        self.signatures.len() >= self.required_signatures as usize
    }
    
    pub fn get_signatures_count(&self) -> usize {
        self.signatures.len()
    }
    
    pub fn get_missing_signatures(&self) -> u32 {
        (self.required_signatures as usize).saturating_sub(self.signatures.len()) as u32
    }
    
    pub fn finalize_transaction(&self) -> Result<Transaction> {
        if !self.is_complete() {
            return Err(QtcError::Multisig(format!(
                "Not enough signatures: have {}, need {}",
                self.signatures.len(),
                self.required_signatures
            )));
        }
        
        let mut tx = self.transaction.clone();
        
        // Create the signature script for multisig
        let mut signature_script = Vec::new();
        
        // OP_0 (due to off-by-one bug in OP_CHECKMULTISIG)
        signature_script.push(0x00);
        
        // Add signatures in order
        let mut signature_indices: Vec<_> = self.signatures.keys().cloned().collect();
        signature_indices.sort();
        
        for &index in &signature_indices[..self.required_signatures as usize] {
            let partial_sig = &self.signatures[&index];
            let sig_bytes = partial_sig.signature.to_bytes();
            signature_script.push(sig_bytes.len() as u8);
            signature_script.extend_from_slice(&sig_bytes);
        }
        
        // Add the redeem script
        let redeem_script = self.script.get_redeem_script();
        signature_script.push(redeem_script.len() as u8);
        signature_script.extend_from_slice(redeem_script);
        
        // Update the input's signature script
        if self.input_index < tx.inputs.len() {
            tx.inputs[self.input_index].signature_script = signature_script;
        }
        
        Ok(tx)
    }
    
    pub fn export_partial_signatures(&self) -> Vec<(usize, Signature)> {
        self.signatures.iter()
            .map(|(&index, partial_sig)| (index, partial_sig.signature.clone()))
            .collect()
    }
    
    pub fn import_partial_signatures(&mut self, signatures: Vec<(usize, Signature)>) -> Result<usize> {
        let mut added = 0;
        
        for (signer_index, signature) in signatures {
            if !self.signatures.contains_key(&signer_index) {
                self.add_signature(signer_index, signature)?;
                added += 1;
            }
        }
        
        Ok(added)
    }
    
    pub fn to_psbt(&self) -> Result<Vec<u8>> {
        // Simplified PSBT (Partially Signed Bitcoin Transaction) export
        // In production, would use proper PSBT library
        
        let mut psbt_data = Vec::new();
        
        // PSBT magic bytes
        psbt_data.extend_from_slice(b"psbt");
        psbt_data.push(0xff);
        
        // Serialize transaction (simplified)
        let tx_bytes = bincode::serialize(&self.transaction)
            .map_err(|e| QtcError::Multisig(format!("Failed to serialize transaction: {}", e)))?;
        
        psbt_data.extend_from_slice(&(tx_bytes.len() as u32).to_le_bytes());
        psbt_data.extend_from_slice(&tx_bytes);
        
        // Add partial signatures
        for (index, partial_sig) in &self.signatures {
            psbt_data.push(*index as u8);
            let sig_bytes = partial_sig.signature.to_bytes();
            psbt_data.push(sig_bytes.len() as u8);
            psbt_data.extend_from_slice(&sig_bytes);
        }
        
        Ok(psbt_data)
    }
    
    pub fn from_psbt(psbt_data: &[u8], _script: MultisigScript) -> Result<Self> {
        // Simplified PSBT parsing
        if psbt_data.len() < 5 || &psbt_data[0..4] != b"psbt" || psbt_data[4] != 0xff {
            return Err(QtcError::Multisig("Invalid PSBT format".to_string()));
        }
        
        // This is a simplified implementation
        // Production code would use proper PSBT parsing
        
        Err(QtcError::Multisig("PSBT parsing not fully implemented".to_string()))
    }
}

// Utility functions for multisig operations
pub struct MultisigUtils;

impl MultisigUtils {
    pub fn validate_multisig_params(required: u32, total: u32) -> Result<()> {
        if required == 0 {
            return Err(QtcError::Multisig("Required signatures cannot be zero".to_string()));
        }
        
        if required > total {
            return Err(QtcError::Multisig("Required signatures cannot exceed total keys".to_string()));
        }
        
        if total > 20 {
            return Err(QtcError::Multisig("Too many keys (maximum 20)".to_string()));
        }
        
        Ok(())
    }
    
    pub fn sort_public_keys(mut public_keys: Vec<PublicKey>) -> Vec<PublicKey> {
        // Sort public keys lexicographically for canonical ordering
        public_keys.sort_by(|a, b| a.to_bytes().cmp(b.to_bytes()));
        public_keys
    }
    
    pub fn estimate_multisig_size(required: u32, total: u32) -> usize {
        // Estimate the size of a multisig transaction
        let base_size = 10; // Version, lock time, etc.
        let input_size = 180; // Average input size
        let output_size = 34; // Average output size
        let signature_size = 73; // Average signature size
        let pubkey_size = 33; // Compressed public key size
        
        let script_size = 1 + (required as usize * signature_size) + 1 + (total as usize * pubkey_size) + 2;
        
        base_size + input_size + script_size + (2 * output_size)
    }
    
    pub fn calculate_multisig_fee(required: u32, total: u32, fee_rate: u64) -> u64 {
        let size = Self::estimate_multisig_size(required, total) as u64;
        size * fee_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::KeyPair;
    
    #[test]
    fn test_2_of_3_multisig() -> Result<()> {
        let key1 = KeyPair::new()?.public_key;
        let key2 = KeyPair::new()?.public_key;
        let key3 = KeyPair::new()?.public_key;
        
        let multisig = MultisigWallet::create_2_of_3(
            "test_multisig".to_string(),
            key1,
            key2,
            key3,
            0,
        )?;
        
        assert_eq!(multisig.required_signatures(), 2);
        assert_eq!(multisig.total_keys(), 3);
        assert!(multisig.address.starts_with("qtc"));
        
        Ok(())
    }
    
    #[test]
    fn test_multisig_script_creation() -> Result<()> {
        let key1 = KeyPair::new()?.public_key;
        let key2 = KeyPair::new()?.public_key;
        
        let script = MultisigScript::new(2, vec![key1, key2])?;
        
        assert_eq!(script.required_signatures, 2);
        assert_eq!(script.total_keys, 2);
        assert!(!script.script.is_empty());
        
        Ok(())
    }
    
    #[test]
    fn test_signature_collector() -> Result<()> {
        let key1 = KeyPair::new()?;
        let key2 = KeyPair::new()?;
        let key3 = KeyPair::new()?;
        
        let script = MultisigScript::new(2, vec![
            key1.public_key.clone(),
            key2.public_key.clone(),
            key3.public_key.clone(),
        ])?;
        
        let tx = Transaction::new();
        let mut collector = SignatureCollector::new(tx, 0, script);
        
        assert!(!collector.is_complete());
        assert_eq!(collector.get_missing_signatures(), 2);
        
        collector.sign_with_key(0, &key1.private_key)?;
        assert_eq!(collector.get_signatures_count(), 1);
        assert_eq!(collector.get_missing_signatures(), 1);
        
        collector.sign_with_key(1, &key2.private_key)?;
        assert!(collector.is_complete());
        assert_eq!(collector.get_missing_signatures(), 0);
        
        let _finalized_tx = collector.finalize_transaction()?;
        
        Ok(())
    }
    
    #[test]
    fn test_multisig_validation() {
        assert!(MultisigUtils::validate_multisig_params(2, 3).is_ok());
        assert!(MultisigUtils::validate_multisig_params(0, 3).is_err());
        assert!(MultisigUtils::validate_multisig_params(4, 3).is_err());
        assert!(MultisigUtils::validate_multisig_params(1, 21).is_err());
    }
}
