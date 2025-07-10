use crate::crypto::keys::{PrivateKey, PublicKey, KeyPair};
use crate::crypto::hash::Hash256;
use crate::{QtcError, Result};
use bip39::{Mnemonic as Bip39Mnemonic, Language};
use bitcoin::bip32::{Xpriv, Xpub, DerivationPath, ChildNumber};
use bitcoin::Network;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Mnemonic {
    inner: Bip39Mnemonic,
}

#[derive(Debug, Clone)]
pub struct Seed {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdWallet {
    pub master_key: Vec<u8>, // Serialized Xpriv
    pub mnemonic_hash: Hash256, // Hash of mnemonic for verification
    pub account_index: u32,
    pub next_external_index: u32,
    pub next_internal_index: u32, // Change addresses
}

impl Mnemonic {
    pub fn new(word_count: u32) -> Result<Self> {
        // Generate random entropy for the mnemonic
        let entropy_size = match word_count {
            12 => 16,
            15 => 20,
            18 => 24,
            21 => 28,
            24 => 32,
            _ => return Err(QtcError::Wallet("Invalid word count".to_string())),
        };
        
        let mut entropy = vec![0u8; entropy_size];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut entropy);
        
        let mnemonic = Bip39Mnemonic::from_entropy(&entropy)
            .map_err(|e| QtcError::Wallet(format!("Failed to generate mnemonic: {}", e)))?;
        Ok(Self { inner: mnemonic })
    }
    
    pub fn from_phrase(phrase: &str) -> Result<Self> {
        let mnemonic = Bip39Mnemonic::parse_in_normalized(Language::English, phrase)
            .map_err(|e| QtcError::Wallet(format!("Invalid mnemonic phrase: {}", e)))?;
        
        Ok(Self { inner: mnemonic })
    }
    
    pub fn phrase(&self) -> String {
        self.inner.to_string()
    }
    
    pub fn word_count(&self) -> usize {
        self.inner.word_count()
    }
    
    pub fn words(&self) -> Vec<String> {
        self.inner.to_string().split_whitespace().map(|s| s.to_string()).collect()
    }
    
    pub fn to_seed(&self, passphrase: &str) -> Seed {
        let seed_bytes = self.inner.to_seed(passphrase);
        Seed {
            bytes: seed_bytes.to_vec(),
        }
    }
    
    pub fn validate_phrase(phrase: &str) -> bool {
        Bip39Mnemonic::parse_in_normalized(Language::English, phrase).is_ok()
    }
    
    pub fn hash(&self) -> Hash256 {
        Hash256::hash(self.phrase().as_bytes())
    }
}

impl Seed {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    
    pub fn to_master_key(&self) -> Result<Xpriv> {
        Xpriv::new_master(Network::Bitcoin, &self.bytes)
            .map_err(|e| QtcError::Wallet(format!("Failed to create master key: {}", e)))
    }
}

impl HdWallet {
    pub fn new(mnemonic: &Mnemonic, passphrase: &str) -> Result<Self> {
        let seed = mnemonic.to_seed(passphrase);
        let master_key = seed.to_master_key()?;
        
        Ok(Self {
            master_key: master_key.encode().to_vec(),
            mnemonic_hash: mnemonic.hash(),
            account_index: 0,
            next_external_index: 0,
            next_internal_index: 0,
        })
    }
    
    pub fn from_mnemonic_phrase(phrase: &str, passphrase: &str) -> Result<Self> {
        let mnemonic = Mnemonic::from_phrase(phrase)?;
        Self::new(&mnemonic, passphrase)
    }
    
    pub fn get_master_key(&self) -> Result<Xpriv> {
        let bytes: [u8; 78] = self.master_key.as_slice().try_into()
            .map_err(|_| QtcError::Wallet("Invalid master key length".to_string()))?;
        Xpriv::decode(&bytes)
            .map_err(|e| QtcError::Wallet(format!("Failed to decode master key: {}", e)))
    }
    
    pub fn derive_account_key(&self, account: u32) -> Result<Xpriv> {
        let master_key = self.get_master_key()?;
        let secp = secp256k1::Secp256k1::new();
        
        // BIP44 path: m/44'/coin_type'/account'
        // Using coin type 0 for now (Bitcoin's, should be registered)
        let path = format!("m/44'/0'/{}'", account);
        let derivation_path = DerivationPath::from_str(&path)
            .map_err(|e| QtcError::Wallet(format!("Invalid derivation path: {}", e)))?;
        
        master_key.derive_priv(&secp, &derivation_path)
            .map_err(|e| QtcError::Wallet(format!("Failed to derive account key: {}", e)))
    }
    
    pub fn derive_address_key(&self, account: u32, change: bool, index: u32) -> Result<Xpriv> {
        let account_key = self.derive_account_key(account)?;
        let secp = secp256k1::Secp256k1::new();
        
        // BIP44 path: m/44'/coin_type'/account'/change/index
        let change_value = if change { 1 } else { 0 };
        let path = vec![
            ChildNumber::from_normal_idx(change_value)?,
            ChildNumber::from_normal_idx(index)?,
        ];
        
        account_key.derive_priv(&secp, &path)
            .map_err(|e| QtcError::Wallet(format!("Failed to derive address key: {}", e)))
    }
    
    pub fn get_next_address(&mut self, change: bool) -> Result<(String, u32)> {
        let index = if change {
            self.next_internal_index
        } else {
            self.next_external_index
        };
        
        let extended_key = self.derive_address_key(self.account_index, change, index)?;
        let private_key = PrivateKey::from_bytes(&extended_key.private_key.secret_bytes())?;
        let public_key = private_key.public_key()?;
        let address = public_key.to_address();
        
        // Increment the appropriate index
        if change {
            self.next_internal_index += 1;
        } else {
            self.next_external_index += 1;
        }
        
        Ok((address, index))
    }
    
    pub fn get_address_at_index(&self, change: bool, index: u32) -> Result<String> {
        let extended_key = self.derive_address_key(self.account_index, change, index)?;
        let private_key = PrivateKey::from_bytes(&extended_key.private_key.secret_bytes())?;
        let public_key = private_key.public_key()?;
        Ok(public_key.to_address())
    }
    
    pub fn get_private_key_for_address(&self, change: bool, index: u32) -> Result<PrivateKey> {
        let extended_key = self.derive_address_key(self.account_index, change, index)?;
        PrivateKey::from_bytes(&extended_key.private_key.secret_bytes())
    }
    
    pub fn scan_for_addresses(&self, max_gap: u32) -> Result<Vec<(String, bool, u32)>> {
        let mut addresses = Vec::new();
        
        // Scan external addresses (receiving)
        for index in 0..max_gap {
            let address = self.get_address_at_index(false, index)?;
            addresses.push((address, false, index));
        }
        
        // Scan internal addresses (change)
        for index in 0..max_gap {
            let address = self.get_address_at_index(true, index)?;
            addresses.push((address, true, index));
        }
        
        Ok(addresses)
    }
    
    pub fn export_xprv(&self) -> Result<String> {
        let master_key = self.get_master_key()?;
        Ok(master_key.to_string())
    }
    
    pub fn export_xpub(&self) -> Result<String> {
        let master_key = self.get_master_key()?;
        let secp = secp256k1::Secp256k1::new();
        let public_key = Xpub::from_priv(&secp, &master_key);
        Ok(public_key.to_string())
    }
}

// Utility functions for mnemonic generation and validation
pub struct MnemonicUtils;

impl MnemonicUtils {
    pub fn generate_12_word() -> Result<Mnemonic> {
        Mnemonic::new(12)
    }
    
    pub fn generate_24_word() -> Result<Mnemonic> {
        Mnemonic::new(24)
    }
    
    pub fn validate_word(word: &str) -> bool {
        bip39::Language::English.word_list().contains(&word)
    }
    
    pub fn suggest_words(prefix: &str) -> Vec<String> {
        bip39::Language::English.word_list()
            .iter()
            .filter(|word| word.starts_with(prefix))
            .map(|word| word.to_string())
            .collect()
    }
    
    pub fn calculate_checksum(entropy: &[u8]) -> Result<String> {
        // Calculate BIP39 checksum
        let checksum_bits = entropy.len() * 8 / 32;
        let hash = Hash256::hash(entropy);
        let checksum_byte = hash.as_bytes()[0];
        
        let mut result = String::new();
        for i in 0..checksum_bits {
            if checksum_byte & (0x80 >> i) != 0 {
                result.push('1');
            } else {
                result.push('0');
            }
        }
        
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mnemonic_generation() -> Result<()> {
        let mnemonic = Mnemonic::new(MnemonicType::Words12)?;
        assert_eq!(mnemonic.word_count(), 12);
        
        let phrase = mnemonic.phrase();
        assert!(Mnemonic::validate_phrase(&phrase));
        
        Ok(())
    }
    
    #[test]
    fn test_seed_generation() -> Result<()> {
        let mnemonic = Mnemonic::new(MnemonicType::Words12)?;
        let seed1 = mnemonic.to_seed("");
        let seed2 = mnemonic.to_seed("password");
        
        assert_ne!(seed1.as_bytes(), seed2.as_bytes());
        assert_eq!(seed1.as_bytes().len(), 64);
        
        Ok(())
    }
    
    #[test]
    fn test_hd_wallet() -> Result<()> {
        let mnemonic = Mnemonic::new(MnemonicType::Words12)?;
        let mut wallet = HdWallet::new(&mnemonic, "")?;
        
        let (address1, index1) = wallet.get_next_address(false)?;
        let (address2, index2) = wallet.get_next_address(false)?;
        
        assert_ne!(address1, address2);
        assert_eq!(index1, 0);
        assert_eq!(index2, 1);
        
        // Test deterministic generation
        let address1_again = wallet.get_address_at_index(false, 0)?;
        assert_eq!(address1, address1_again);
        
        Ok(())
    }
    
    #[test]
    fn test_mnemonic_roundtrip() -> Result<()> {
        let mnemonic = Mnemonic::new(MnemonicType::Words12)?;
        let phrase = mnemonic.phrase();
        let restored_mnemonic = Mnemonic::from_phrase(&phrase)?;
        
        assert_eq!(mnemonic.phrase(), restored_mnemonic.phrase());
        
        Ok(())
    }
}
