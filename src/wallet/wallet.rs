use crate::core::{Transaction, TxInput, TxOutput, OutPoint};
use crate::core::Blockchain;
use crate::crypto::keys::{PrivateKey, PublicKey, KeyPair};
use crate::crypto::hash::Hash256;
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletAddress {
    pub address: String,
    pub private_key: Option<Vec<u8>>, // Encrypted private key
    pub public_key: Vec<u8>,
    pub derivation_path: Option<String>,
    pub is_change: bool,
    pub used: bool,
}

#[derive(Debug)]
pub struct Wallet {
    pub info: WalletInfo,
    addresses: HashMap<String, WalletAddress>,
    hd_wallet: Option<HdWallet>,
    db: Arc<Database>,
    blockchain: Arc<Blockchain>,
}

impl Wallet {
    pub fn new_simple(name: String, db: Arc<Database>, blockchain: Arc<Blockchain>) -> Result<Self> {
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
    
    pub fn new_hd(name: String, mnemonic: &Mnemonic, passphrase: &str, db: Arc<Database>, blockchain: Arc<Blockchain>) -> Result<Self> {
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
    
    pub fn from_mnemonic_phrase(name: String, phrase: &str, passphrase: &str, db: Arc<Database>, blockchain: Arc<Blockchain>) -> Result<Self> {
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
            let balance = self.blockchain.get_balance(address)?;
            total_balance += balance;
        }
        
        Ok(total_balance)
    }
    
    pub fn get_address_balance(&self, address: &str) -> Result<u64> {
        if !self.addresses.contains_key(address) {
            return Err(QtcError::Wallet("Address not found in wallet".to_string()));
        }
        
        self.blockchain.get_balance(address)
    }
    
    pub fn get_addresses(&self) -> Vec<String> {
        self.addresses.keys().cloned().collect()
    }
    
    pub fn get_unused_address(&self) -> Option<String> {
        self.addresses.values()
            .find(|addr| !addr.used && !addr.is_change)
            .map(|addr| addr.address.clone())
    }
    
    pub fn get_change_address(&mut self) -> Result<String> {
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
            };
            
            self.addresses.insert(address.clone(), wallet_address);
            self.save()?;
            
            Ok(address)
        } else {
            // For simple wallets, reuse existing address
            Ok(self.addresses.keys().next().unwrap().clone())
        }
    }
    
    pub fn create_transaction(&mut self, to_address: &str, amount: u64, fee_rate: u64) -> Result<Transaction> {
        let mut builder = TransactionBuilder::new(self);
        builder.add_output(to_address, amount)?;
        builder.set_fee_rate(fee_rate);
        builder.build()
    }
    
    pub fn sign_transaction(&self, tx: &mut Transaction) -> Result<()> {
        for (index, input) in tx.inputs.iter_mut().enumerate() {
            // Find the private key for this input
            if let Some(private_key) = self.find_private_key_for_input(input)? {
                let signature_hash = tx.get_signature_hash(index);
                let signature = private_key.sign(&signature_hash)?;
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
    
    fn find_private_key_for_input(&self, input: &TxInput) -> Result<Option<PrivateKey>> {
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
        self.db.save_wallet(&self.info.name, self)
    }
    
    pub fn load(name: &str, db: Arc<Database>, blockchain: Arc<Blockchain>) -> Result<Self> {
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
}

pub struct TransactionBuilder<'a> {
    wallet: &'a mut Wallet,
    outputs: Vec<(String, u64)>,
    fee_rate: u64, // satoshis per byte
    selected_utxos: Vec<(OutPoint, u64)>,
}

impl<'a> TransactionBuilder<'a> {
    pub fn new(wallet: &'a mut Wallet) -> Self {
        Self {
            wallet,
            outputs: Vec::new(),
            fee_rate: 1000, // Default 0.00001 QTC per byte
            selected_utxos: Vec::new(),
        }
    }
    
    pub fn add_output(&mut self, address: &str, amount: u64) -> Result<()> {
        if !crate::crypto::keys::is_valid_address(address) {
            return Err(QtcError::Wallet("Invalid recipient address".to_string()));
        }
        
        self.outputs.push((address.to_string(), amount));
        Ok(())
    }
    
    pub fn set_fee_rate(&mut self, fee_rate: u64) {
        self.fee_rate = fee_rate;
    }
    
    pub fn build(&mut self) -> Result<Transaction> {
        let total_output_amount: u64 = self.outputs.iter().map(|(_, amount)| amount).sum();
        
        // Select UTXOs
        self.select_utxos(total_output_amount)?;
        
        let total_input_amount: u64 = self.selected_utxos.iter().map(|(_, amount)| amount).sum();
        
        // Calculate fee (simplified)
        let estimated_size = 250; // Rough estimate
        let fee = self.fee_rate * estimated_size;
        
        if total_input_amount < total_output_amount + fee {
            return Err(QtcError::InsufficientFunds {
                required: total_output_amount + fee,
                available: total_input_amount,
            });
        }
        
        // Create transaction
        let mut tx = Transaction::new();
        
        // Add inputs
        for (outpoint, _) in &self.selected_utxos {
            tx.add_input(outpoint.clone(), vec![]); // Empty signature script for now
        }
        
        // Add outputs
        for (address, amount) in &self.outputs {
            tx.add_output(*amount, address);
        }
        
        // Add change output if needed
        let change_amount = total_input_amount - total_output_amount - fee;
        if change_amount > 0 {
            let change_address = self.wallet.get_change_address()?;
            tx.add_output(change_amount, &change_address);
        }
        
        // Sign transaction
        self.wallet.sign_transaction(&mut tx)?;
        
        Ok(tx)
    }
    
    fn select_utxos(&mut self, target_amount: u64) -> Result<()> {
        // Get all UTXOs for wallet addresses
        let mut all_utxos = Vec::new();
        
        for address in self.wallet.get_addresses() {
            let utxos = self.wallet.blockchain.get_utxos(&address)?;
            for (txid, vout, amount) in utxos {
                all_utxos.push((OutPoint::new(txid, vout), amount));
            }
        }
        
        // Sort by amount (largest first)
        all_utxos.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Select UTXOs until we have enough
        let mut selected_amount = 0u64;
        for (outpoint, amount) in all_utxos {
            self.selected_utxos.push((outpoint, amount));
            selected_amount += amount;
            
            if selected_amount >= target_amount {
                break;
            }
        }
        
        if selected_amount < target_amount {
            return Err(QtcError::InsufficientFunds {
                required: target_amount,
                available: selected_amount,
            });
        }
        
        Ok(())
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
        let blockchain = Arc::new(Blockchain::new(db.clone())?);
        
        let wallet = Wallet::new_simple("test_wallet".to_string(), db, blockchain)?;
        
        assert_eq!(wallet.info.wallet_type, WalletType::Simple);
        assert_eq!(wallet.addresses.len(), 1);
        
        Ok(())
    }
    
    #[test]
    fn test_hd_wallet_creation() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let blockchain = Arc::new(Blockchain::new(db.clone())?);
        
        let mnemonic = Mnemonic::new(bip39::MnemonicType::Words12)?;
        let wallet = Wallet::new_hd("test_hd_wallet".to_string(), &mnemonic, "", db, blockchain)?;
        
        assert!(matches!(wallet.info.wallet_type, WalletType::HD));
        assert!(wallet.addresses.len() > 0);
        
        Ok(())
    }
}
