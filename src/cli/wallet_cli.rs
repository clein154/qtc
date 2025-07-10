use crate::cli::commands::{WalletCommands, MultisigCommands};
use crate::core::Blockchain;
use crate::storage::Database;
use crate::wallet::{Wallet, WalletInfo};
use crate::wallet::wallet::WalletType;
use crate::wallet::bip39::{Mnemonic, MnemonicUtils};
use crate::wallet::multisig::{MultisigWallet, MultisigUtils, SignatureCollector};
use crate::crypto::keys::{PrivateKey, is_valid_address};
use crate::crypto::hash::Hashable;
use crate::{QtcError, Result};
use dialoguer::{Input, Password, Confirm, Select, theme::ColorfulTheme};
use console::{style, Emoji};
use std::sync::{Arc, RwLock};

static WALLET: Emoji<'_, '_> = Emoji("💼", "");
static KEY: Emoji<'_, '_> = Emoji("🔑", "");
static COIN: Emoji<'_, '_> = Emoji("🪙", "");
static ARROW: Emoji<'_, '_> = Emoji("➡️", "");
static CHECK: Emoji<'_, '_> = Emoji("✅", "");
static CROSS: Emoji<'_, '_> = Emoji("❌", "");

pub struct WalletCli {
    db: Arc<Database>,
    blockchain: Arc<RwLock<Blockchain>>,
}

impl WalletCli {
    pub fn new(db: Arc<Database>, blockchain: Arc<RwLock<Blockchain>>) -> Self {
        Self { db, blockchain }
    }
    
    pub async fn handle_command(&mut self, command: WalletCommands) -> Result<()> {
        match command {
            WalletCommands::Create { name, hd, words24, passphrase } => {
                self.create_wallet(name, hd, words24, passphrase).await
            }
            
            WalletCommands::Import { name, mnemonic, passphrase } => {
                self.import_wallet(name, mnemonic, passphrase).await
            }
            
            WalletCommands::ImportKey { name, wif } => {
                self.import_key_wallet(name, wif).await
            }
            
            WalletCommands::List => {
                self.list_wallets().await
            }
            
            WalletCommands::Info { name } => {
                self.wallet_info(name).await
            }
            
            WalletCommands::Balance { name, detailed } => {
                self.wallet_balance(name, detailed).await
            }
            
            WalletCommands::NewAddress { name, change } => {
                self.new_address(name, change).await
            }
            
            WalletCommands::Addresses { name, unused } => {
                self.list_addresses(name, unused).await
            }
            
            WalletCommands::Send { wallet, to, amount, fee_rate, yes } => {
                self.send_transaction(wallet, to, amount, fee_rate, yes).await
            }
            
            WalletCommands::History { name, limit } => {
                self.transaction_history(name, limit).await
            }
            
            WalletCommands::Export { name, format } => {
                self.export_wallet(name, format).await
            }
            
            WalletCommands::Multisig { command } => {
                self.handle_multisig_command(command).await
            }
            
            WalletCommands::Backup { name, path } => {
                self.backup_wallet(name, path).await
            }
        }
    }
    
    async fn create_wallet(&self, name: String, hd: bool, words24: bool, passphrase: Option<String>) -> Result<()> {
        println!("{} {} Creating new wallet: {}", WALLET, style("QTC Wallet").bold().cyan(), style(&name).bold());
        
        // Check if wallet already exists
        if self.db.list_wallets()?.contains(&name) {
            println!("{} Wallet '{}' already exists!", CROSS, name);
            return Ok(());
        }
        
        if hd {
            // Create HD wallet with BIP39 mnemonic
            let word_count = if words24 { 24 } else { 12 };
            
            let mnemonic = Mnemonic::new(word_count)?;
            let passphrase = passphrase.unwrap_or_else(|| {
                Password::with_theme(&ColorfulTheme::default())
                    .with_prompt("Enter passphrase (optional, press Enter for none)")
                    .allow_empty_password(true)
                    .interact()
                    .unwrap_or_default()
            });
            
            println!("\n{} {} Your BIP39 mnemonic phrase:", KEY, style("IMPORTANT").bold().red());
            println!("{}", style(&mnemonic.phrase()).bold().yellow());
            println!("\n{} {}", 
                style("WARNING:").bold().red(), 
                "Write down this mnemonic phrase and store it safely!"
            );
            println!("This is the ONLY way to recover your wallet!");
            
            if !Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Have you written down the mnemonic phrase?")
                .interact()
                .map_err(|e| QtcError::Wallet(format!("Interaction error: {}", e)))?
            {
                println!("{} Wallet creation cancelled", CROSS);
                return Ok(());
            }
            
            let wallet = Wallet::new_hd(name.clone(), &mnemonic, &passphrase, self.db.clone(), self.blockchain.clone())?;
            wallet.save()?;
            
            println!("{} HD wallet '{}' created successfully!", CHECK, name);
            println!("Addresses generated: {}", wallet.info.address_count);
            
        } else {
            // Create simple wallet
            let wallet = Wallet::new_simple(name.clone(), self.db.clone(), self.blockchain.clone())?;
            let address = wallet.get_addresses()[0].clone();
            wallet.save()?;
            
            println!("{} Simple wallet '{}' created successfully!", CHECK, name);
            println!("Address: {}", style(address).bold().green());
        }
        
        Ok(())
    }
    
    async fn import_wallet(&self, name: String, mnemonic: Option<String>, passphrase: Option<String>) -> Result<()> {
        println!("{} {} Importing wallet: {}", WALLET, style("QTC Wallet").bold().cyan(), style(&name).bold());
        
        // Check if wallet already exists
        if self.db.list_wallets()?.contains(&name) {
            println!("{} Wallet '{}' already exists!", CROSS, name);
            return Ok(());
        }
        
        let mnemonic_phrase = mnemonic.unwrap_or_else(|| {
            Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter BIP39 mnemonic phrase")
                .interact_text()
                .unwrap()
        });
        
        // Validate mnemonic
        if !Mnemonic::validate_phrase(&mnemonic_phrase) {
            println!("{} Invalid mnemonic phrase!", CROSS);
            return Ok(());
        }
        
        let passphrase = passphrase.unwrap_or_else(|| {
            Password::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter passphrase (optional)")
                .allow_empty_password(true)
                .interact()
                .unwrap_or_default()
        });
        
        let wallet = Wallet::from_mnemonic_phrase(name.clone(), &mnemonic_phrase, &passphrase, self.db.clone(), self.blockchain.clone())?;
        wallet.save()?;
        
        println!("{} Wallet '{}' imported successfully!", CHECK, name);
        println!("Addresses found: {}", wallet.info.address_count);
        
        Ok(())
    }
    
    async fn import_key_wallet(&self, name: String, wif: String) -> Result<()> {
        println!("{} {} Importing wallet from private key: {}", WALLET, style("QTC Wallet").bold().cyan(), style(&name).bold());
        
        // Check if wallet already exists
        if self.db.list_wallets()?.contains(&name) {
            println!("{} Wallet '{}' already exists!", CROSS, name);
            return Ok(());
        }
        
        // Validate private key
        let _private_key = PrivateKey::from_wif(&wif)?;
        
        let mut wallet = Wallet::new_simple(name.clone(), self.db.clone(), self.blockchain.clone())?;
        let address = wallet.import_private_key(&wif)?;
        
        println!("{} Wallet '{}' imported successfully!", CHECK, name);
        println!("Address: {}", style(address).bold().green());
        
        Ok(())
    }
    
    async fn list_wallets(&self) -> Result<()> {
        println!("{} {} Available Wallets:", WALLET, style("QTC Wallet").bold().cyan());
        
        let wallets = self.db.list_wallets()?;
        
        if wallets.is_empty() {
            println!("No wallets found. Create one with: qtcd wallet create <name>");
            return Ok(());
        }
        
        for wallet_name in wallets {
            // Try to load wallet info
            match self.db.load_wallet(&wallet_name, self.blockchain.clone()) {
                Ok(wallet) => {
                    let balance = wallet.get_balance().unwrap_or(0);
                    let wallet_type = match wallet.info.wallet_type {
                        WalletType::Simple => "Simple",
                        WalletType::HD => "HD (BIP39)",
                        WalletType::Multisig { required, total } => {
                            // Format as string to avoid borrowing issues
                            return Ok(());
                        }
                        WalletType::WatchOnly => "Watch-Only",
                    };
                    
                    println!("  {} {} ({}) - Balance: {:.8} QTC", 
                        COIN,
                        style(&wallet_name).bold(),
                        wallet_type,
                        balance as f64 / 100_000_000.0
                    );
                }
                Err(_) => {
                    println!("  {} {} (Error loading)", CROSS, style(&wallet_name).red());
                }
            }
        }
        
        Ok(())
    }
    
    async fn wallet_info(&self, name: String) -> Result<()> {
        let wallet = self.db.load_wallet(&name, self.blockchain.clone())?;
        
        println!("{} {} Wallet Information: {}", WALLET, style("QTC Wallet").bold().cyan(), style(&name).bold());
        println!("Type: {:?}", wallet.info.wallet_type);
        println!("Created: {}", chrono::DateTime::from_timestamp(wallet.info.created_at as i64, 0).unwrap().format("%Y-%m-%d %H:%M:%S"));
        println!("Encrypted: {}", wallet.info.is_encrypted);
        println!("Address count: {}", wallet.info.address_count);
        
        let balance = wallet.get_balance()?;
        println!("Balance: {:.8} QTC", balance as f64 / 100_000_000.0);
        
        Ok(())
    }
    
    async fn wallet_balance(&self, name: String, detailed: bool) -> Result<()> {
        let wallet = self.db.load_wallet(&name, self.blockchain.clone())?;
        let balance = wallet.get_balance()?;
        
        println!("{} {} Balance for wallet: {}", COIN, style("QTC Wallet").bold().cyan(), style(&name).bold());
        println!("Total: {:.8} QTC", balance as f64 / 100_000_000.0);
        
        if detailed {
            println!("\n{} UTXO Breakdown:", style("Detailed").bold());
            let addresses = wallet.get_addresses();
            
            for address in addresses {
                let addr_balance = wallet.get_address_balance(&address)?;
                if addr_balance > 0 {
                    println!("  {}: {:.8} QTC", 
                        style(&address).dim(),
                        addr_balance as f64 / 100_000_000.0
                    );
                    
                    // Show UTXOs for this address
                    let utxos = wallet.blockchain.read().unwrap().get_utxos(&address)?;
                    for (txid, vout, value) in utxos {
                        println!("    {}:{} - {:.8} QTC", 
                            hex::encode(&txid.as_bytes()[0..8]),
                            vout,
                            value as f64 / 100_000_000.0
                        );
                    }
                }
            }
        }
        
        Ok(())
    }
    
    async fn new_address(&self, name: String, change: bool) -> Result<()> {
        let mut wallet = self.db.load_wallet(&name, self.blockchain.clone())?;
        
        let address = if change {
            wallet.get_change_address()?
        } else {
            match wallet.hd_wallet.as_mut() {
                Some(hd_wallet) => {
                    let (addr, _) = hd_wallet.get_next_address(false)?;
                    wallet.save()?;
                    addr
                }
                None => {
                    // Simple wallet - return existing address
                    wallet.get_addresses()[0].clone()
                }
            }
        };
        
        let addr_type = if change { "Change" } else { "Receiving" };
        println!("{} {} {} address for wallet '{}': {}", 
            KEY, 
            style("New").bold().green(),
            addr_type,
            name,
            style(address).bold().cyan()
        );
        
        Ok(())
    }
    
    async fn list_addresses(&self, name: String, unused: bool) -> Result<()> {
        let wallet = self.db.load_wallet(&name, self.blockchain.clone())?;
        
        println!("{} {} Addresses for wallet: {}", KEY, style("QTC Wallet").bold().cyan(), style(&name).bold());
        
        let addresses = wallet.get_addresses();
        
        for address in addresses {
            let balance = wallet.get_address_balance(&address)?;
            let has_balance = balance > 0;
            
            if unused && has_balance {
                continue;
            }
            
            let status = if has_balance {
                style(format!("{:.8} QTC", balance as f64 / 100_000_000.0)).green()
            } else {
                style("Unused".to_string()).dim()
            };
            
            println!("  {} - {}", style(&address).cyan(), status);
        }
        
        Ok(())
    }
    
    async fn send_transaction(&self, wallet_name: String, to: String, amount_str: String, fee_rate: Option<u64>, yes: bool) -> Result<()> {
        let mut wallet = self.db.load_wallet(&wallet_name, self.blockchain.clone())?;
        
        // Validate recipient address
        if !is_valid_address(&to) {
            println!("{} Invalid recipient address: {}", CROSS, to);
            return Ok(());
        }
        
        // Parse amount
        let amount = match amount_str.parse::<f64>() {
            Ok(amount) => (amount * 100_000_000.0) as u64,
            Err(_) => {
                println!("{} Invalid amount: {}", CROSS, amount_str);
                return Ok(());
            }
        };
        
        // Check balance
        let balance = wallet.get_balance()?;
        if balance < amount {
            println!("{} Insufficient funds: have {:.8} QTC, need {:.8} QTC", 
                CROSS,
                balance as f64 / 100_000_000.0,
                amount as f64 / 100_000_000.0
            );
            return Ok(());
        }
        
        let fee_rate = fee_rate.unwrap_or(1000); // Default 0.00001 QTC per byte
        
        println!("{} {} Preparing transaction:", ARROW, style("QTC Wallet").bold().cyan());
        println!("From wallet: {}", style(&wallet_name).bold());
        println!("To address: {}", style(&to).bold().cyan());
        println!("Amount: {:.8} QTC", amount as f64 / 100_000_000.0);
        println!("Fee rate: {} sat/byte", fee_rate);
        
        if !yes {
            if !Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Confirm transaction?")
                .interact()
                .map_err(|e| QtcError::Wallet(format!("Interaction error: {}", e)))?
            {
                println!("{} Transaction cancelled", CROSS);
                return Ok(());
            }
        }
        
        // Create transaction
        match wallet.create_transaction(&to, amount, fee_rate) {
            Ok(tx) => {
                println!("{} Transaction created successfully!", CHECK);
                println!("Transaction ID: {}", hex::encode(tx.hash().as_bytes()));
                println!("(Broadcasting not implemented in this demo)");
            }
            Err(e) => {
                println!("{} Failed to create transaction: {}", CROSS, e);
            }
        }
        
        Ok(())
    }
    
    async fn transaction_history(&self, name: String, limit: Option<usize>) -> Result<()> {
        let wallet = self.db.load_wallet(&name, self.blockchain.clone())?;
        let _limit = limit.unwrap_or(10);
        
        println!("{} {} Transaction history for wallet: {}", COIN, style("QTC Wallet").bold().cyan(), style(&name).bold());
        
        // Get transaction history
        let history = wallet.get_transaction_history()?;
        
        if history.is_empty() {
            println!("No transactions found.");
            return Ok(());
        }
        
        for (hash, tx, height) in history {
            let tx_type = if tx.is_coinbase() { "Coinbase" } else { "Transfer" };
            println!("  {} {} (Block {}): {}", 
                COIN,
                tx_type,
                height,
                hex::encode(&hash.as_bytes()[0..8])
            );
        }
        
        Ok(())
    }
    
    async fn export_wallet(&self, name: String, format: Option<String>) -> Result<()> {
        let wallet = self.db.load_wallet(&name, self.blockchain.clone())?;
        let format = format.unwrap_or_else(|| {
            let options = vec!["mnemonic", "wif", "descriptor"];
            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("Export format")
                .items(&options)
                .default(0)
                .interact()
                .unwrap();
            options[selection].to_string()
        });
        
        println!("{} {} Exporting wallet: {}", KEY, style("QTC Wallet").bold().cyan(), style(&name).bold());
        
        match format.as_str() {
            "mnemonic" => {
                if let Some(hd_wallet) = &wallet.hd_wallet {
                    let xprv = hd_wallet.export_xprv()?;
                    println!("Extended Private Key: {}", style(xprv).yellow());
                    println!("\n{} Keep this private key secure!", style("WARNING:").bold().red());
                } else {
                    println!("{} Not an HD wallet - cannot export mnemonic", CROSS);
                }
            }
            
            "wif" => {
                let addresses = wallet.get_addresses();
                for address in addresses {
                    if let Ok(wif) = wallet.export_private_key(&address) {
                        println!("Address: {}", address);
                        println!("Private Key (WIF): {}", style(wif).yellow());
                        println!();
                    }
                }
            }
            
            "descriptor" => {
                println!("Descriptor export not yet implemented");
            }
            
            _ => {
                println!("{} Invalid export format. Use: mnemonic, wif, or descriptor", CROSS);
            }
        }
        
        Ok(())
    }
    
    async fn handle_multisig_command(&self, command: MultisigCommands) -> Result<()> {
        match command {
            MultisigCommands::Create { name, required, pubkeys, our_keys } => {
                self.create_multisig_wallet(name, required, pubkeys, our_keys).await
            }
            
            MultisigCommands::Import { name, descriptor, our_keys } => {
                self.import_multisig_wallet(name, descriptor, our_keys).await
            }
            
            MultisigCommands::Sign { wallet, tx_hex, input_index } => {
                self.sign_multisig_transaction(wallet, tx_hex, input_index).await
            }
            
            MultisigCommands::Finalize { wallet, tx_hex, signatures } => {
                self.finalize_multisig_transaction(wallet, tx_hex, signatures).await
            }
        }
    }
    
    async fn create_multisig_wallet(&self, name: String, required: u32, pubkey_strings: Vec<String>, our_keys: Vec<usize>) -> Result<()> {
        println!("{} {} Creating multisig wallet: {}", WALLET, style("QTC Multisig").bold().magenta(), style(&name).bold());
        
        // Validate parameters
        if let Err(e) = MultisigUtils::validate_multisig_params(required, pubkey_strings.len() as u32) {
            println!("{} {}", CROSS, e);
            return Ok(());
        }
        
        // Parse public keys
        let mut public_keys = Vec::new();
        for pubkey_hex in pubkey_strings {
            let pubkey_bytes = hex::decode(&pubkey_hex)
                .map_err(|_| QtcError::Multisig("Invalid public key hex".to_string()))?;
            let pubkey = crate::crypto::keys::PublicKey::from_bytes(&pubkey_bytes)?;
            public_keys.push(pubkey);
        }
        
        // Create multisig wallet
        let multisig_wallet = MultisigWallet::new(name.clone(), required, public_keys, our_keys)?;
        
        println!("{} Multisig wallet created successfully!", CHECK);
        println!("Required signatures: {}/{}", required, multisig_wallet.total_keys());
        println!("Address: {}", style(&multisig_wallet.address).bold().cyan());
        println!("Descriptor: {}", multisig_wallet.export_descriptor());
        
        // Save wallet (would need to implement persistence for multisig wallets)
        
        Ok(())
    }
    
    async fn import_multisig_wallet(&self, name: String, descriptor: String, our_keys: Vec<usize>) -> Result<()> {
        println!("{} {} Importing multisig wallet: {}", WALLET, style("QTC Multisig").bold().magenta(), style(&name).bold());
        
        let multisig_wallet = MultisigWallet::from_descriptor(name, &descriptor, our_keys)?;
        
        println!("{} Multisig wallet imported successfully!", CHECK);
        println!("Required signatures: {}/{}", multisig_wallet.required_signatures(), multisig_wallet.total_keys());
        println!("Address: {}", style(&multisig_wallet.address).bold().cyan());
        
        Ok(())
    }
    
    async fn sign_multisig_transaction(&self, _wallet: String, _tx_hex: String, _input_index: usize) -> Result<()> {
        println!("{} Multisig transaction signing not yet fully implemented", style("INFO").bold().blue());
        Ok(())
    }
    
    async fn finalize_multisig_transaction(&self, _wallet: String, _tx_hex: String, _signatures: Vec<String>) -> Result<()> {
        println!("{} Multisig transaction finalization not yet fully implemented", style("INFO").bold().blue());
        Ok(())
    }
    
    async fn backup_wallet(&self, name: String, path: String) -> Result<()> {
        let _wallet = self.db.load_wallet(&name, self.blockchain.clone())?;
        
        println!("{} {} Creating backup for wallet: {}", KEY, style("QTC Wallet").bold().cyan(), style(&name).bold());
        println!("Backup path: {}", style(&path).bold());
        
        // Implementation would export wallet data to file
        println!("{} Wallet backup functionality not yet implemented", style("INFO").bold().blue());
        
        Ok(())
    }
}
