use crate::core::{Transaction, TxInput};
// use crate::core::transaction::OutPoint;
// use crate::crypto::hash::Hashable;
use crate::core::Blockchain;
use crate::crypto::keys::{PrivateKey, KeyPair};
use crate::crypto::hash::Hash256;
use crate::crypto::pqc::{PqcKeyPair};
use crate::storage::Database;
use crate::wallet::bip39::{HdWallet, Mnemonic};
use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    pub name: String,
    pub wallet_type: WalletType,
    pub created_at: u64,
    pub last_used: u64,
    pub is_encrypted: bool,
    pub balance: u64,
    pub address_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WalletType {
    Simple,
    HD,
    Multisig { required: u32, total: u32 },
    WatchOnly,
    PostQuantum,
    HybridClassicPqc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletAddress {
    pub address: String,
    pub private_key: Option<Vec<u8>>, // Encrypted private key
    pub public_key: Vec<u8>,
    pub derivation_path: Option<String>,
    pub is_change: bool,
    pub used: bool,
    pub address_type: AddressType,
    pub pqc_data: Option<PqcAddressData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AddressType {
    Classic,
    PostQuantum,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqcAddressData {
    pub signing_private_key: Option<Vec<u8>>,
    pub encryption_private_key: Option<Vec<u8>>,
    pub signing_public_key: Vec<u8>,
    pub encryption_public_key: Vec<u8>,
}

#[derive(Debug)]
pub struct Wallet {
    pub info: WalletInfo,
    pub addresses: HashMap<String, WalletAddress>,
    pub hd_wallet: Option<HdWallet>,
    pub db: Arc<Database>,
    pub blockchain: Arc<std::sync::RwLock<Blockchain>>,
}

impl Wallet {
    pub fn new_simple(name: String, db: Arc<Database>, blockchain: Arc<std::sync::RwLock<Blockchain>>) -> Result<Self> {
        let keypair = KeyPair::new()?;
        let address = keypair.address();
        
        let mut addresses = HashMap::new();
        addresses.insert(address.clone(), WalletAddress {
            address: address.clone(),
            private_key: Some(keypair.private_key.to_bytes().to_vec()),
            public_key: keypair.public_key.to_bytes().to_vec(),
            derivation_path: None,
            is_change: false,
            used: false,
            address_type: AddressType::Classic,
            pqc_data: None,
        });
        
        let info = WalletInfo {
            name,
            wallet_type: WalletType::Simple,
            created_at: chrono::Utc::now().timestamp() as u64,
            last_used: 0,
            is_encrypted: false,
            balance: 0,
            address_count: 1,
        };
        
        Ok(Self {
            info,
            addresses,
            hd_wallet: None,
            db,
            blockchain,
        })
    }
    
    pub fn new_hd(name: String, mnemonic: &Mnemonic, passphrase: &str, db: Arc<Database>, blockchain: Arc<std::sync::RwLock<Blockchain>>) -> Result<Self> {
        let hd_wallet = HdWallet::new(mnemonic, passphrase)?;
        
        let info = WalletInfo {
            name,
            wallet_type: WalletType::HD,
            created_at: chrono::Utc::now().timestamp() as u64,
            last_used: 0,
            is_encrypted: false,
            balance: 0,
            address_count: 0,
        };
        
        let mut wallet = Self {
            info,
            addresses: HashMap::new(),
            hd_wallet: Some(hd_wallet),
            db,
            blockchain,
        };
        
        // Generate initial addresses
        wallet.generate_addresses(10)?;
        
        Ok(wallet)
    }
    
    pub fn from_mnemonic_phrase(name: String, phrase: &str, passphrase: &str, db: Arc<Database>, blockchain: Arc<std::sync::RwLock<Blockchain>>) -> Result<Self> {
        let mnemonic = Mnemonic::from_phrase(phrase)?;
        Self::new_hd(name, &mnemonic, passphrase, db, blockchain)
    }
    
    pub fn generate_addresses(&mut self, count: u32) -> Result<Vec<String>> {
        let hd_wallet = self.hd_wallet.as_mut()
            .ok_or_else(|| QtcError::Wallet("Not an HD wallet".to_string()))?;
        
        let mut new_addresses = Vec::new();
        
        for _ in 0..count {
            let (address, index) = hd_wallet.get_next_address(false)?;
            let private_key = hd_wallet.get_private_key_for_address(false, index)?;
            let public_key = private_key.public_key()?;
            
            let wallet_address = WalletAddress {
                address: address.clone(),
                private_key: Some(private_key.to_bytes().to_vec()),
                public_key: public_key.to_bytes().to_vec(),
                derivation_path: Some(format!("m/44'/0'/0'/0/{}", index)),
                is_change: false,
                used: false,
                address_type: AddressType::Classic,
                pqc_data: None,
            };
            
            self.addresses.insert(address.clone(), wallet_address);
            new_addresses.push(address);
        }
        
        self.info.address_count += count;
        self.save()?;
        
        Ok(new_addresses)
    }
    
    pub fn get_balance(&self) -> Result<u64> {
        let mut total_balance = 0u64;
        
        for address in self.addresses.keys() {
            let balance = {
                let blockchain = self.blockchain.read().unwrap();
                blockchain.get_balance(address)?
            };
            total_balance += balance;
        }
        
        Ok(total_balance)
    }
    
    pub fn get_address_balance(&self, address: &str) -> Result<u64> {
        if !self.addresses.contains_key(address) {
            return Err(QtcError::Wallet("Address not found in wallet".to_string()));
        }
        
        let blockchain = self.blockchain.read().unwrap();
        blockchain.get_balance(address)
    }
    
    pub fn get_addresses(&self) -> Vec<String> {
        self.addresses.keys().cloned().collect()
    }
    
    pub fn get_unused_address(&self) -> Option<String> {
        self.addresses.values()
            .find(|addr| !addr.used && !addr.is_change)
            .map(|addr| addr.address.clone())
    }
    
    pub fn get_change_address(&self) -> Result<String> {
        // For simple wallets, reuse existing address
        if self.hd_wallet.is_none() {
            return Ok(self.addresses.keys().next().unwrap().clone());
        }
        
        // For HD wallets, we would need a mutable reference to generate new addresses
        // For now, return the first available address
        Ok(self.addresses.keys().next().unwrap().clone())
    }
    
    pub fn get_change_address_mut(&mut self) -> Result<String> {
        if let Some(hd_wallet) = &mut self.hd_wallet {
            let (address, index) = hd_wallet.get_next_address(true)?;
            let private_key = hd_wallet.get_private_key_for_address(true, index)?;
            let public_key = private_key.public_key()?;
            
            let wallet_address = WalletAddress {
                address: address.clone(),
                private_key: Some(private_key.to_bytes().to_vec()),
                public_key: public_key.to_bytes().to_vec(),
                derivation_path: Some(format!("m/44'/0'/0'/1/{}", index)),
                is_change: true,
                used: false,
                address_type: AddressType::Classic,
                pqc_data: None,
            };
            
            self.addresses.insert(address.clone(), wallet_address);
            self.save()?;
            
            Ok(address)
        } else {
            // For simple wallets, reuse existing address
            Ok(self.addresses.keys().next().unwrap().clone())
        }
    }
    
    pub fn create_transaction(&self, to_address: &str, amount: u64, fee_rate: u64) -> Result<Transaction> {
        // Use the TransactionBuilder from core::transaction module
        let mut builder = crate::core::transaction::TransactionBuilder::new(self);
        builder.add_output(to_address, amount)?;
        builder.set_fee_rate(fee_rate);
        builder.build()
    }
    
    pub fn sign_transaction(&self, tx: &mut Transaction) -> Result<()> {
        // Pre-calculate signature hashes to avoid borrowing issues
        let signature_hashes: Vec<_> = (0..tx.inputs.len())
            .map(|index| tx.get_signature_hash(index))
            .collect();
        
        for (index, input) in tx.inputs.iter_mut().enumerate() {
            // Find the private key for this input
            if let Some(private_key) = self.find_private_key_for_input(input)? {
                let signature_hash = &signature_hashes[index];
                let signature = private_key.sign(signature_hash)?;
                let public_key = private_key.public_key()?;
                
                // Create signature script (simplified)
                let mut script = Vec::new();
                script.extend_from_slice(&signature.to_bytes());
                script.extend_from_slice(public_key.to_bytes());
                
                input.signature_script = script;
            }
        }
        
        Ok(())
    }
    
    fn find_private_key_for_input(&self, _input: &TxInput) -> Result<Option<PrivateKey>> {
        // This would need to look up the output being spent to determine the address
        // For now, simplified implementation
        for addr_info in self.addresses.values() {
            if let Some(private_key_bytes) = &addr_info.private_key {
                let private_key = PrivateKey::from_bytes(private_key_bytes)?;
                return Ok(Some(private_key));
            }
        }
        
        Ok(None)
    }
    
    pub fn mark_address_used(&mut self, address: &str) -> Result<()> {
        if let Some(addr_info) = self.addresses.get_mut(address) {
            addr_info.used = true;
            self.save()?;
        }
        
        Ok(())
    }
    
    pub fn save(&self) -> Result<()> {
        self.db.save_wallet_complete(self)
    }
    
    pub fn load(name: &str, db: Arc<Database>, blockchain: Arc<std::sync::RwLock<Blockchain>>) -> Result<Self> {
        db.load_wallet(name, blockchain)
    }
    
    pub fn export_private_key(&self, address: &str) -> Result<String> {
        let addr_info = self.addresses.get(address)
            .ok_or_else(|| QtcError::Wallet("Address not found".to_string()))?;
        
        let private_key_bytes = addr_info.private_key.as_ref()
            .ok_or_else(|| QtcError::Wallet("No private key available (watch-only?)".to_string()))?;
        
        let private_key = PrivateKey::from_bytes(private_key_bytes)?;
        Ok(private_key.to_wif())
    }
    
    pub fn import_private_key(&mut self, wif: &str) -> Result<String> {
        let private_key = PrivateKey::from_wif(wif)?;
        let public_key = private_key.public_key()?;
        let address = public_key.to_address();
        
        let wallet_address = WalletAddress {
            address: address.clone(),
            private_key: Some(private_key.to_bytes().to_vec()),
            public_key: public_key.to_bytes().to_vec(),
            derivation_path: None,
            is_change: false,
            used: false,
            address_type: AddressType::Classic,
            pqc_data: None,
        };
        
        self.addresses.insert(address.clone(), wallet_address);
        self.info.address_count += 1;
        self.save()?;
        
        Ok(address)
    }
    
    pub fn get_transaction_history(&self) -> Result<Vec<(Hash256, Transaction, u64)>> {
        // This would need to scan the blockchain for transactions involving our addresses
        // Simplified implementation for now
        Ok(Vec::new())
    }

    /// Create a new Post-Quantum Cryptography wallet
    pub fn new_pqc(name: String, db: Arc<Database>, blockchain: Arc<std::sync::RwLock<Blockchain>>) -> Result<Self> {
        let pqc_keypair = PqcKeyPair::new()?;
        let pqc_address = pqc_keypair.address();
        
        let pqc_data = PqcAddressData {
            signing_private_key: Some(pqc_keypair.signing_private_key_bytes()),
            encryption_private_key: Some(pqc_keypair.encryption_private_key_bytes()),
            signing_public_key: pqc_address.signing_public_key.clone(),
            encryption_public_key: pqc_address.encryption_public_key.clone(),
        };

        let mut addresses = HashMap::new();
        addresses.insert(pqc_address.address.clone(), WalletAddress {
            address: pqc_address.address.clone(),
            private_key: None, // PQC private keys stored separately
            public_key: pqc_address.signing_public_key.clone(),
            derivation_path: None,
            is_change: false,
            used: false,
            address_type: AddressType::PostQuantum,
            pqc_data: Some(pqc_data),
        });
        
        let info = WalletInfo {
            name,
            wallet_type: WalletType::PostQuantum,
            created_at: chrono::Utc::now().timestamp() as u64,
            last_used: 0,
            is_encrypted: false,
            balance: 0,
            address_count: 1,
        };
        
        Ok(Self {
            info,
            addresses,
            hd_wallet: None,
            db,
            blockchain,
        })
    }

    /// Generate a new PQC address for HD wallets
    pub fn generate_pqc_address(&mut self) -> Result<String> {
        let pqc_keypair = PqcKeyPair::new()?;
        let pqc_address = pqc_keypair.address();
        
        let pqc_data = PqcAddressData {
            signing_private_key: Some(pqc_keypair.signing_private_key_bytes()),
            encryption_private_key: Some(pqc_keypair.encryption_private_key_bytes()),
            signing_public_key: pqc_address.signing_public_key.clone(),
            encryption_public_key: pqc_address.encryption_public_key.clone(),
        };

        let wallet_address = WalletAddress {
            address: pqc_address.address.clone(),
            private_key: None, // PQC private keys stored separately
            public_key: pqc_address.signing_public_key.clone(),
            derivation_path: None,
            is_change: false,
            used: false,
            address_type: AddressType::PostQuantum,
            pqc_data: Some(pqc_data),
        };
        
        self.addresses.insert(pqc_address.address.clone(), wallet_address);
        self.info.address_count += 1;
        self.save()?;
        
        Ok(pqc_address.address)
    }

    /// Get addresses by type (Classic, PostQuantum, or Hybrid)
    pub fn get_addresses_by_type(&self, address_type: AddressType) -> Vec<String> {
        self.addresses.values()
            .filter(|addr| addr.address_type == address_type)
            .map(|addr| addr.address.clone())
            .collect()
    }

    /// Check if wallet has post-quantum addresses
    pub fn has_pqc_addresses(&self) -> bool {
        self.addresses.values().any(|addr| matches!(addr.address_type, AddressType::PostQuantum))
    }

    /// Create a new hybrid wallet (both classic and PQC)
    pub fn new_hybrid(name: String, db: Arc<Database>, blockchain: Arc<std::sync::RwLock<Blockchain>>) -> Result<Self> {
        let mut addresses = HashMap::new();
        
        // Create classic keypair
        let classic_keypair = KeyPair::new()?;
        let classic_address = classic_keypair.address();
        
        // Add classic address
        addresses.insert(classic_address.clone(), WalletAddress {
            address: classic_address.clone(),
            private_key: Some(classic_keypair.private_key.to_bytes().to_vec()),
            public_key: classic_keypair.public_key.to_bytes().to_vec(),
            derivation_path: None,
            is_change: false,
            used: false,
            address_type: AddressType::Classic,
            pqc_data: None,
        });
        
        // Create PQC keypair
        let pqc_keypair = PqcKeyPair::new()?;
        let pqc_address = pqc_keypair.address();
        
        let pqc_data = PqcAddressData {
            signing_private_key: Some(pqc_keypair.signing_private_key_bytes()),
            encryption_private_key: Some(pqc_keypair.encryption_private_key_bytes()),
            signing_public_key: pqc_address.signing_public_key.clone(),
            encryption_public_key: pqc_address.encryption_public_key.clone(),
        };

        // Add PQC address
        addresses.insert(pqc_address.address.clone(), WalletAddress {
            address: pqc_address.address.clone(),
            private_key: None, // PQC private keys stored separately
            public_key: pqc_address.signing_public_key.clone(),
            derivation_path: None,
            is_change: false,
            used: false,
            address_type: AddressType::PostQuantum,
            pqc_data: Some(pqc_data),
        });
        
        let info = WalletInfo {
            name,
            wallet_type: WalletType::HybridClassicPqc,
            created_at: chrono::Utc::now().timestamp() as u64,
            last_used: 0,
            is_encrypted: false,
            balance: 0,
            address_count: 2,
        };
        
        Ok(Self {
            info,
            addresses,
            hd_wallet: None,
            db,
            blockchain,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use crate::core::Blockchain;
    use tempfile::TempDir;
    use std::sync::Arc;
    
    #[test]
    fn test_simple_wallet_creation() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let blockchain = Arc::new(std::sync::RwLock::new(Blockchain::new(db.clone())?));
        
        let wallet = Wallet::new_simple("test_wallet".to_string(), db, blockchain)?;
        
        assert_eq!(wallet.info.wallet_type, WalletType::Simple);
        assert_eq!(wallet.addresses.len(), 1);
        
        Ok(())
    }
    
    #[test]
    fn test_hd_wallet_creation() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let blockchain = Arc::new(std::sync::RwLock::new(Blockchain::new(db.clone())?));
        
        let mnemonic = Mnemonic::new(12)?;
        let wallet = Wallet::new_hd("test_hd_wallet".to_string(), &mnemonic, "", db, blockchain)?;
        
        assert!(matches!(wallet.info.wallet_type, WalletType::HD));
        assert!(wallet.addresses.len() > 0);
        
        Ok(())
    }
}