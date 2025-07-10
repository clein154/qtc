use crate::crypto::hash::{Hash256, Hashable};
use crate::crypto::signatures::Signature;
use crate::crypto::keys::{PublicKey, PrivateKey};
use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub lock_time: u64,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInput {
    pub previous_output: OutPoint,
    pub signature_script: Vec<u8>,
    pub sequence: u32,
    pub witness: Vec<Vec<u8>>, // For future segwit support
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxOutput {
    pub value: u64,
    pub script_pubkey: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct OutPoint {
    pub txid: Hash256,
    pub vout: u32,
}

impl Transaction {
    pub fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            lock_time: 0,
            version: 1,
        }
    }
    
    pub fn new_coinbase(address: String, value: u64, message: String) -> Self {
        let script_pubkey = Self::address_to_script_pubkey(&address);
        
        let coinbase_input = TxInput {
            previous_output: OutPoint {
                txid: Hash256::zero(),
                vout: 0xFFFFFFFF,
            },
            signature_script: message.into_bytes(),
            sequence: 0xFFFFFFFF,
            witness: Vec::new(),
        };
        
        let output = TxOutput {
            value,
            script_pubkey,
        };
        
        Self {
            inputs: vec![coinbase_input],
            outputs: vec![output],
            lock_time: 0,
            version: 1,
        }
    }
    
    pub fn add_input(&mut self, outpoint: OutPoint, signature_script: Vec<u8>) {
        let input = TxInput {
            previous_output: outpoint,
            signature_script,
            sequence: 0xFFFFFFFF,
            witness: Vec::new(),
        };
        self.inputs.push(input);
    }
    
    pub fn add_output(&mut self, value: u64, address: &str) {
        let script_pubkey = Self::address_to_script_pubkey(address);
        let output = TxOutput {
            value,
            script_pubkey,
        };
        self.outputs.push(output);
    }
    
    pub fn is_coinbase(&self) -> bool {
        self.inputs.len() == 1 
            && self.inputs[0].previous_output.txid == Hash256::zero()
            && self.inputs[0].previous_output.vout == 0xFFFFFFFF
    }
    
    pub fn total_input_value(&self) -> u64 {
        // This would need UTXO lookup in real implementation
        // For now, return 0 for coinbase transactions
        if self.is_coinbase() {
            0
        } else {
            // TODO: Look up UTXO values
            0
        }
    }
    
    pub fn total_output_value(&self) -> u64 {
        self.outputs.iter().map(|output| output.value).sum()
    }
    
    pub fn fee(&self) -> u64 {
        if self.is_coinbase() {
            0
        } else {
            self.total_input_value().saturating_sub(self.total_output_value())
        }
    }
    
    pub fn size(&self) -> usize {
        bincode::serialize(self).map(|data| data.len()).unwrap_or(0)
    }
    
    pub fn get_signature_hash(&self, input_index: usize) -> Hash256 {
        // Simplified signature hash for SIGHASH_ALL
        let mut data = Vec::new();
        
        // Add version
        data.extend_from_slice(&self.version.to_le_bytes());
        
        // Add inputs (without signature scripts)
        data.extend_from_slice(&(self.inputs.len() as u32).to_le_bytes());
        for (i, input) in self.inputs.iter().enumerate() {
            data.extend_from_slice(input.previous_output.txid.as_bytes());
            data.extend_from_slice(&input.previous_output.vout.to_le_bytes());
            
            if i == input_index {
                // Use the script_pubkey of the output being spent
                // For now, use empty script
                data.extend_from_slice(&0u32.to_le_bytes()); // empty script length
            } else {
                data.extend_from_slice(&0u32.to_le_bytes()); // empty script
            }
            
            data.extend_from_slice(&input.sequence.to_le_bytes());
        }
        
        // Add outputs
        data.extend_from_slice(&(self.outputs.len() as u32).to_le_bytes());
        for output in &self.outputs {
            data.extend_from_slice(&output.value.to_le_bytes());
            data.extend_from_slice(&(output.script_pubkey.len() as u32).to_le_bytes());
            data.extend_from_slice(&output.script_pubkey);
        }
        
        // Add lock_time
        data.extend_from_slice(&self.lock_time.to_le_bytes());
        
        // Add SIGHASH_ALL
        data.extend_from_slice(&1u32.to_le_bytes());
        
        Hash256::hash(&data)
    }
    
    fn address_to_script_pubkey(address: &str) -> Vec<u8> {
        // Simplified script creation
        // In real implementation, this would decode the address and create proper scripts
        let mut script = Vec::new();
        script.push(0x76); // OP_DUP
        script.push(0xa9); // OP_HASH160
        script.push(20);   // Push 20 bytes
        
        // For now, just hash the address string
        let hash = Hash256::hash(address.as_bytes());
        script.extend_from_slice(&hash.as_bytes()[0..20]);
        
        script.push(0x88); // OP_EQUALVERIFY
        script.push(0xac); // OP_CHECKSIG
        
        script
    }
    
    pub fn verify_signature(&self, input_index: usize, public_key: &PublicKey) -> Result<bool> {
        if input_index >= self.inputs.len() {
            return Err(QtcError::Transaction("Invalid input index".to_string()));
        }
        
        let input = &self.inputs[input_index];
        if input.signature_script.is_empty() {
            return Ok(false);
        }
        
        // Extract signature from script (simplified)
        if input.signature_script.len() < 64 {
            return Ok(false);
        }
        
        let signature_bytes = &input.signature_script[input.signature_script.len()-64..];
        let signature = Signature::from_bytes(signature_bytes)?;
        
        let message_hash = self.get_signature_hash(input_index);
        
        Ok(public_key.verify(&message_hash, &signature)?)
    }
}

impl Hashable for Transaction {
    fn hash(&self) -> Hash256 {
        let mut data = Vec::new();
        
        // Add version
        data.extend_from_slice(&self.version.to_le_bytes());
        
        // Add inputs
        data.extend_from_slice(&(self.inputs.len() as u32).to_le_bytes());
        for input in &self.inputs {
            data.extend_from_slice(input.previous_output.txid.as_bytes());
            data.extend_from_slice(&input.previous_output.vout.to_le_bytes());
            data.extend_from_slice(&(input.signature_script.len() as u32).to_le_bytes());
            data.extend_from_slice(&input.signature_script);
            data.extend_from_slice(&input.sequence.to_le_bytes());
        }
        
        // Add outputs
        data.extend_from_slice(&(self.outputs.len() as u32).to_le_bytes());
        for output in &self.outputs {
            data.extend_from_slice(&output.value.to_le_bytes());
            data.extend_from_slice(&(output.script_pubkey.len() as u32).to_le_bytes());
            data.extend_from_slice(&output.script_pubkey);
        }
        
        // Add lock_time
        data.extend_from_slice(&self.lock_time.to_le_bytes());
        
        Hash256::hash(&data)
    }
}

impl OutPoint {
    pub fn new(txid: Hash256, vout: u32) -> Self {
        Self { txid, vout }
    }
    
    pub fn is_null(&self) -> bool {
        self.txid == Hash256::zero() && self.vout == 0xFFFFFFFF
    }
}

/// Transaction builder for creating new transactions
#[derive(Debug)]
pub struct TransactionBuilder<'a> {
    wallet: &'a crate::wallet::Wallet,
    outputs: Vec<TxOutput>,
    fee_rate: u64,
    estimated_size: usize,
}

impl<'a> TransactionBuilder<'a> {
    pub fn new(wallet: &'a crate::wallet::Wallet) -> Self {
        Self {
            wallet,
            outputs: Vec::new(),
            fee_rate: 1000, // Default: 1000 satoshis per byte
            estimated_size: 0,
        }
    }
    
    pub fn add_output(&mut self, address: &str, amount: u64) -> Result<()> {
        let script_pubkey = Transaction::address_to_script_pubkey(address);
        let output = TxOutput {
            value: amount,
            script_pubkey,
        };
        self.outputs.push(output);
        self.update_estimated_size();
        Ok(())
    }
    
    pub fn set_fee_rate(&mut self, fee_rate: u64) {
        self.fee_rate = fee_rate;
    }
    
    fn update_estimated_size(&mut self) {
        // Estimate transaction size
        // Base size: version(4) + input_count(1-9) + output_count(1-9) + lock_time(4)
        let mut size = 4 + 1 + 1 + 4;
        
        // Add estimated input size: outpoint(36) + script_length(1-9) + script(~107) + sequence(4)
        // Conservative estimate: 148 bytes per input
        let estimated_inputs = 2; // Conservative estimate
        size += estimated_inputs * 148;
        
        // Add output sizes
        for output in &self.outputs {
            size += 8 + 1 + output.script_pubkey.len(); // value(8) + script_length(1-9) + script
        }
        
        self.estimated_size = size;
    }
    
    pub fn build(&mut self) -> Result<Transaction> {
        if self.outputs.is_empty() {
            return Err(QtcError::Transaction("No outputs specified".to_string()));
        }
        
        let total_output_value: u64 = self.outputs.iter().map(|o| o.value).sum();
        let estimated_fee = self.fee_rate * self.estimated_size as u64 / 1000; // Fee rate is per 1000 bytes
        let total_needed = total_output_value + estimated_fee;
        
        // Find UTXOs to spend
        let addresses = self.wallet.get_addresses();
        let mut available_utxos = Vec::new();
        let mut total_available = 0u64;
        
        // Get blockchain reference
        let blockchain = self.wallet.blockchain.read().unwrap();
        
        for address in &addresses {
            let utxos = blockchain.get_utxos(address)?;
            for (txid, vout, value) in utxos {
                available_utxos.push((txid, vout, value, address.clone()));
                total_available += value;
            }
        }
        
        if total_available < total_needed {
            return Err(QtcError::Transaction(format!(
                "Insufficient funds: have {:.8} QTC, need {:.8} QTC",
                total_available as f64 / 100_000_000.0,
                total_needed as f64 / 100_000_000.0
            )));
        }
        
        // Select UTXOs (simple greedy algorithm)
        available_utxos.sort_by(|a, b| b.2.cmp(&a.2)); // Sort by value descending
        let mut selected_utxos = Vec::new();
        let mut selected_value = 0u64;
        
        for (txid, vout, value, address) in available_utxos {
            selected_utxos.push((txid, vout, value, address));
            selected_value += value;
            if selected_value >= total_needed {
                break;
            }
        }
        
        // Create transaction
        let mut tx = Transaction::new();
        
        // Add inputs
        for (txid, vout, _value, _address) in &selected_utxos {
            tx.add_input(OutPoint::new(*txid, *vout), Vec::new()); // Empty signature script for now
        }
        
        // Add outputs
        for output in &self.outputs {
            tx.outputs.push(output.clone());
        }
        
        // Add change output if needed
        let actual_fee = self.fee_rate * tx.size() as u64 / 1000;
        let change_amount = selected_value.saturating_sub(total_output_value + actual_fee);
        
        if change_amount > 546 { // Dust threshold
            let change_address = self.wallet.get_change_address().unwrap_or_else(|_| {
                addresses.first().unwrap_or(&"unknown".to_string()).clone()
            });
            tx.add_output(change_amount, &change_address);
        }
        
        // Sign the transaction
        self.sign_transaction(&mut tx, &selected_utxos)?;
        
        Ok(tx)
    }
    
    fn sign_transaction(&self, tx: &mut Transaction, selected_utxos: &[(Hash256, u32, u64, String)]) -> Result<()> {
        for (input_index, (_, _, _, address)) in selected_utxos.iter().enumerate() {
            // Get private key for this address
            if let Ok(private_key_wif) = self.wallet.export_private_key(address) {
                let private_key = PrivateKey::from_wif(&private_key_wif)?;
                let public_key = private_key.public_key()?;
                
                // Sign the input
                let signature_hash = tx.get_signature_hash(input_index);
                let signature = private_key.sign(&signature_hash)?;
                
                // Create signature script (simplified P2PKH)
                let mut script = Vec::new();
                
                // Add signature
                let sig_bytes = signature.to_bytes();
                script.push(sig_bytes.len() as u8);
                script.extend_from_slice(&sig_bytes);
                script.push(0x01); // SIGHASH_ALL
                
                // Add public key
                let pubkey_bytes = public_key.to_bytes();
                script.push(pubkey_bytes.len() as u8);
                script.extend_from_slice(pubkey_bytes);
                
                tx.inputs[input_index].signature_script = script;
            }
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_coinbase_transaction() {
        let tx = Transaction::new_coinbase(
            "qtc1test".to_string(),
            2710000000,
            "Genesis block".to_string(),
        );
        
        assert!(tx.is_coinbase());
        assert_eq!(tx.inputs.len(), 1);
        assert_eq!(tx.outputs.len(), 1);
        assert_eq!(tx.outputs[0].value, 2710000000);
    }
    
    #[test]
    fn test_transaction_hash() {
        let tx = Transaction::new_coinbase(
            "qtc1test".to_string(),
            1000,
            "test".to_string(),
        );
        
        let hash1 = tx.hash();
        let hash2 = tx.hash();
        
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, Hash256::zero());
    }
}
