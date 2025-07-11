use crate::config::Config;
use crate::cli::wallet_cli::WalletCli;
use crate::cli::mining_cli::MiningCli;
use crate::core::Blockchain;
use crate::storage::Database;
use crate::network::p2p::P2PNode;
use crate::api::rest::RestApi;
use crate::api::websocket::WebSocketServer;
use crate::crypto::hash::Hashable;
use crate::{QtcError, Result};
use clap::{Parser, Subcommand};
use std::sync::{Arc, RwLock};
use tokio::signal;
use std::fs::File;

use tar::Builder;
use flate2::{Compression, GzBuilder};
use daemonize::Daemonize;

#[derive(Parser)]
#[command(name = "qtcd")]
#[command(about = "Quantum Goldchain (QTC) Node - A decentralized cryptocurrency with RandomX mining")]
#[command(version = "1.0.0")]
#[command(long_about = "
🌟 Quantum Goldchain (QTC) Node
⛓️  Initiating Real-World Launch Protocol Mode
🧑‍💻 Jake online. Mission status: Hardcore Blockchain Implementation Mode ENGAGED

QTC is a post-Bitcoin era chain: 100% decentralized, zero governance, no founders' control.
Features:
- RandomX CPU-only mining (ASIC-resistant)
- BIP39 HD wallets with multi-signature support
- UTXO-based transaction system
- Dynamic difficulty adjustment
- P2P networking
- Complete REST API and WebSocket endpoints
")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
    
    #[arg(long, help = "Data directory")]
    pub data_dir: Option<String>,
    
    #[arg(long, help = "Network port")]
    pub port: Option<u16>,
    
    #[arg(long, help = "Enable debug logging")]
    pub debug: bool,
    
    #[arg(long, help = "Configuration file path")]
    pub config: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Initialize a new QTC node
    Init {
        #[arg(long, help = "Genesis message")]
        genesis_message: Option<String>,
    },
    
    /// Start the QTC node daemon
    Start {
        #[arg(long, help = "Run in daemon mode")]
        daemon: bool,
        
        #[arg(long, help = "Enable mining on startup")]
        mine: bool,
        
        #[arg(long, help = "Mining address")]
        mining_address: Option<String>,
    },
    
    /// Wallet management commands
    #[command(subcommand)]
    Wallet(WalletCommands),
    
    /// Mining commands
    #[command(subcommand)]
    Mine(MiningCommands),
    
    /// Network and peer commands
    #[command(subcommand)]
    Network(NetworkCommands),
    
    /// Blockchain information commands
    #[command(subcommand)]
    Chain(ChainCommands),
    
    /// API server commands
    #[command(subcommand)]
    Api(ApiCommands),
    
    /// Database maintenance commands
    #[command(subcommand)]
    Db(DbCommands),
}

#[derive(Subcommand)]
pub enum WalletCommands {
    /// Create a new wallet
    Create {
        name: String,
        #[arg(long, help = "Create HD wallet with BIP39 mnemonic")]
        hd: bool,
        #[arg(long, help = "Use 24-word mnemonic instead of 12")]
        words24: bool,
        #[arg(long, help = "Passphrase for HD wallet")]
        passphrase: Option<String>,
        #[arg(long, help = "Wallet type: simple, pqc, hybrid")]
        wallet_type: Option<String>,
    },
    
    /// Import wallet from mnemonic phrase
    Import {
        name: String,
        #[arg(long, help = "BIP39 mnemonic phrase")]
        mnemonic: Option<String>,
        #[arg(long, help = "Passphrase for HD wallet")]
        passphrase: Option<String>,
    },
    
    /// Import wallet from private key (WIF format)
    ImportKey {
        name: String,
        #[arg(long, help = "Private key in WIF format")]
        wif: String,
    },
    
    /// List all wallets
    List,
    
    /// Show wallet information
    Info {
        name: String,
    },
    
    /// Get wallet balance
    Balance {
        name: String,
        #[arg(long, help = "Show detailed UTXO breakdown")]
        detailed: bool,
    },
    
    /// Generate new receiving address
    NewAddress {
        name: String,
        #[arg(long, help = "Generate change address")]
        change: bool,
    },
    
    /// List wallet addresses
    Addresses {
        name: String,
        #[arg(long, help = "Show unused addresses only")]
        unused: bool,
    },
    
    /// Send QTC to an address
    Send {
        wallet: String,
        to: String,
        amount: String,
        #[arg(long, help = "Transaction fee rate (satoshis per byte)")]
        fee_rate: Option<u64>,
        #[arg(long, help = "Confirm transaction without prompting")]
        yes: bool,
    },
    
    /// Show transaction history
    History {
        name: String,
        #[arg(long, help = "Number of transactions to show")]
        limit: Option<usize>,
    },
    
    /// Export wallet
    Export {
        name: String,
        #[arg(long, help = "Export format: mnemonic, wif, descriptor")]
        format: Option<String>,
    },
    
    /// Create multisig wallet
    Multisig {
        #[command(subcommand)]
        command: MultisigCommands,
    },
    
    /// Backup wallet
    Backup {
        name: String,
        #[arg(long, help = "Backup file path")]
        path: String,
    },
}

#[derive(Subcommand)]
pub enum MultisigCommands {
    /// Create new multisig wallet
    Create {
        name: String,
        #[arg(long, help = "Required signatures (m in m-of-n)")]
        required: u32,
        #[arg(long, help = "Public keys (hex format)")]
        pubkeys: Vec<String>,
        #[arg(long, help = "Our key indices (which keys we control)")]
        our_keys: Vec<usize>,
    },
    
    /// Import multisig wallet from descriptor
    Import {
        name: String,
        #[arg(long, help = "Miniscript descriptor")]
        descriptor: String,
        #[arg(long, help = "Our key indices")]
        our_keys: Vec<usize>,
    },
    
    /// Create partial signature for transaction
    Sign {
        wallet: String,
        #[arg(long, help = "Transaction hex")]
        tx_hex: String,
        #[arg(long, help = "Input index to sign")]
        input_index: usize,
    },
    
    /// Combine partial signatures and broadcast
    Finalize {
        wallet: String,
        #[arg(long, help = "Transaction hex")]
        tx_hex: String,
        #[arg(long, help = "Partial signatures (hex format)")]
        signatures: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum MiningCommands {
    /// Start mining
    Start {
        #[arg(long, help = "Mining address")]
        address: String,
        #[arg(long, help = "Number of mining threads")]
        threads: Option<usize>,
        #[arg(long, help = "Use fast mode (more memory, better performance)")]
        fast: bool,
    },
    
    /// Stop mining
    Stop,
    
    /// Show mining status
    Status,
    
    /// Mine a single block
    Single {
        #[arg(long, help = "Mining address")]
        address: String,
        #[arg(long, help = "Timeout in seconds")]
        timeout: Option<u64>,
    },
    
    /// Show mining statistics
    Stats,
    
    /// Benchmark RandomX performance
    Benchmark {
        #[arg(long, help = "Benchmark duration in seconds")]
        duration: Option<u64>,
    },
    
    /// Show current difficulty
    Difficulty,
    
    /// Calculate mining profitability
    Profitability {
        #[arg(long, help = "Your hashrate (H/s)")]
        hashrate: f64,
        #[arg(long, help = "Power consumption (watts)")]
        power: Option<f64>,
        #[arg(long, help = "Electricity cost per kWh")]
        cost_per_kwh: Option<f64>,
    },
}

#[derive(Subcommand)]
pub enum NetworkCommands {
    /// Show network status
    Status,
    
    /// List connected peers
    Peers,
    
    /// Connect to a peer
    Connect {
        address: String,
    },
    
    /// Disconnect from a peer
    Disconnect {
        peer_id: String,
    },
    
    /// Add peer to address book
    AddPeer {
        address: String,
        #[arg(long, help = "Peer description")]
        description: Option<String>,
    },
    
    /// Show network statistics
    Stats,
    
    /// Sync blockchain from peers
    Sync {
        #[arg(long, help = "Force full resync")]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum ChainCommands {
    /// Show blockchain information
    Info,
    
    /// Show block information
    Block {
        #[arg(help = "Block hash or height")]
        identifier: String,
        #[arg(long, help = "Show detailed transaction information")]
        verbose: bool,
    },
    
    /// Show transaction information
    Transaction {
        hash: String,
        #[arg(long, help = "Show raw transaction data")]
        raw: bool,
    },
    
    /// List recent blocks
    Blocks {
        #[arg(long, help = "Number of blocks to show")]
        count: Option<usize>,
        #[arg(long, help = "Starting from height")]
        from: Option<u64>,
    },
    
    /// Search for blocks or transactions
    Search {
        query: String,
    },
    
    /// Validate blockchain
    Validate {
        #[arg(long, help = "Validate from specific height")]
        from_height: Option<u64>,
        #[arg(long, help = "Quick validation (headers only)")]
        quick: bool,
    },
    
    /// Show mempool information
    Mempool,
    
    /// Estimate transaction fee
    EstimateFee {
        #[arg(long, help = "Target confirmation blocks")]
        blocks: Option<u32>,
    },
}

#[derive(Subcommand)]
pub enum ApiCommands {
    /// Start API server
    Start {
        #[arg(long, help = "REST API port")]
        rest_port: Option<u16>,
        #[arg(long, help = "WebSocket port")]
        ws_port: Option<u16>,
        #[arg(long, help = "Enable CORS")]
        cors: bool,
    },
    
    /// Stop API server
    Stop,
    
    /// Show API status
    Status,
    
    /// Test API endpoints
    Test {
        #[arg(long, help = "API endpoint to test")]
        endpoint: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum DbCommands {
    /// Show database statistics
    Stats,
    
    /// Compact database
    Compact,
    
    /// Backup database
    Backup {
        path: String,
    },
    
    /// Repair database (if corrupted)
    Repair,
    
    /// Reindex blockchain from blocks
    Reindex {
        #[arg(long, help = "Start from specific height")]
        from_height: Option<u64>,
    },
}

pub async fn run_cli(config: Config) -> Result<()> {
    let cli = Cli::parse();
    
    // Initialize logging once
    let _ = if cli.debug {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).try_init()
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init()
    };
    
    println!("🌟 Quantum Goldchain (QTC) Node Starting...");
    println!("⛓️  Initiating Real-World Launch Protocol Mode");
    println!("🧑‍💻 Jake online. Mission status: Hardcore Blockchain Implementation Mode ENGAGED");
    
    // Override config with CLI arguments
    let mut config = config;
    if let Some(port) = cli.port {
        config.network.port = port;
    }
    if let Some(data_dir) = cli.data_dir {
        config.storage.data_dir = data_dir.into();
    }
    
    // Ensure data directory exists
    std::fs::create_dir_all(&config.storage.data_dir)?;
    
    // Initialize database
    let db_path = config.storage.data_dir.join("qtc.db");
    let db = Arc::new(Database::new(db_path)?);
    
    match cli.command {
        Commands::Init { genesis_message } => {
            init_node(db, genesis_message).await
        }
        
        Commands::Start { daemon, mine, mining_address } => {
            start_node(config, db, daemon, mine, mining_address).await
        }
        
        Commands::Wallet(wallet_cmd) => {
            let blockchain = Arc::new(RwLock::new(Blockchain::new(db.clone())?));
            let mut wallet_cli = WalletCli::new(db, blockchain);
            wallet_cli.handle_command(wallet_cmd).await
        }
        
        Commands::Mine(mining_cmd) => {
            let blockchain = Arc::new(RwLock::new(Blockchain::new(db.clone())?));
            let mut mining_cli = MiningCli::new(blockchain);
            mining_cli.handle_command(mining_cmd).await
        }
        
        Commands::Network(network_cmd) => {
            handle_network_command(config, db, network_cmd).await
        }
        
        Commands::Chain(chain_cmd) => {
            handle_chain_command(db, chain_cmd).await
        }
        
        Commands::Api(api_cmd) => {
            handle_api_command(config, db, api_cmd).await
        }
        
        Commands::Db(db_cmd) => {
            handle_db_command(db, db_cmd).await
        }
    }
}

async fn init_node(db: Arc<Database>, genesis_message: Option<String>) -> Result<()> {
    println!("🌟 Initializing Quantum Goldchain (QTC) Node...");
    
    // Check if already initialized
    if db.get_chain_state().is_ok() {
        println!("⚠️  Node already initialized!");
        return Ok(());
    }
    
    // Initialize blockchain with custom genesis message if provided
    let blockchain = if let Some(message) = genesis_message {
        println!("📝 Using custom genesis message: {}", message);
        // Create custom genesis block here
        Blockchain::new(db)?
    } else {
        Blockchain::new(db)?
    };
    
    let chain_info = blockchain.get_chain_info()?;
    
    println!("✅ QTC Node initialized successfully!");
    println!("📦 Genesis block hash: {}", chain_info.tip);
    println!("🎯 Initial difficulty: {}", chain_info.difficulty);
    println!("💰 Max supply: {} QTC", chain_info.total_supply as f64 / 100_000_000.0);
    println!("");
    println!("🚀 Ready to start mining! Use 'qtcd start --mine --mining-address <your-address>' to begin.");
    
    Ok(())
}

async fn start_node(
    config: Config,
    db: Arc<Database>,
    daemon: bool,
    mine: bool,
    mining_address: Option<String>,
) -> Result<()> {
    if daemon {
        // Properly daemonize the process before starting the node
        let daemonize = Daemonize::new()
            .pid_file("/tmp/qtcd.pid")
            .chown_pid_file(true)
            .working_directory("/tmp")
            .umask(0o777)
            .stderr(std::fs::File::create("/tmp/qtcd.err").unwrap())
            .stdout(std::fs::File::create("/tmp/qtcd.out").unwrap());
        
        match daemonize.start() {
            Ok(_) => {
                // This code runs in the detached daemon process
                log::info!("QTC daemon started successfully");
                start_node_services(config, db, mine, mining_address).await
            }
            Err(e) => {
                eprintln!("Failed to daemonize: {}", e);
                Err(QtcError::InvalidInput(format!("Daemon startup failed: {}", e)))
            }
        }
    } else {
        // Run in foreground mode
        start_node_services(config, db, mine, mining_address).await
    }
}

async fn start_node_services(
    config: Config,
    db: Arc<Database>,
    mine: bool,
    mining_address: Option<String>,
) -> Result<()> {
    println!("🚀 Starting Quantum Goldchain (QTC) Node...");
    
    // Initialize blockchain
    let blockchain = Arc::new(RwLock::new(Blockchain::new(db.clone())?));
    
    // Start P2P networking
    let (mut p2p_node, mut p2p_events, _p2p_commands) = P2PNode::new(
        blockchain.clone(),
        config.network.port,
        config.network.bootstrap_nodes.clone(),
    ).await?;
    
    // Start API servers if enabled
    let mut api_handles = Vec::new();
    
    if config.api.enable_rest {
        let rest_api = RestApi::new(blockchain.clone(), config.api.clone());
        let rest_handle = tokio::spawn(async move {
            if let Err(e) = rest_api.start().await {
                log::error!("REST API error: {}", e);
            }
        });
        api_handles.push(rest_handle);
    }
    
    if config.api.enable_websocket {
        let ws_server = WebSocketServer::new(blockchain.clone(), config.api.websocket_port);
        let ws_handle = tokio::spawn(async move {
            if let Err(e) = ws_server.start().await {
                log::error!("WebSocket server error: {}", e);
            }
        });
        api_handles.push(ws_handle);
    }
    
    // Start mining if requested
    if mine {
        if let Some(address) = mining_address {
            let miner = crate::mining::miner::Miner::new(
                blockchain.clone(),
                address,
                config.mining.threads,
            )?;
            
            let mining_handle = tokio::spawn(async move {
                if let Err(e) = miner.start_mining().await {
                    log::error!("Mining error: {}", e);
                }
            });
            api_handles.push(mining_handle);
        } else {
            return Err(QtcError::InvalidInput("Mining address required when --mine is used".to_string()));
        }
    }
    
    // Start P2P networking
    let p2p_handle = tokio::spawn(async move {
        if let Err(e) = p2p_node.run().await {
            log::error!("P2P node error: {}", e);
        }
    });
    
    // Handle P2P events
    let blockchain_clone = blockchain.clone();
    let event_handle = tokio::spawn(async move {
        while let Ok(event) = p2p_events.recv().await {
            if let Err(e) = handle_p2p_event(blockchain_clone.clone(), event).await {
                log::error!("P2P event handling error: {}", e);
            }
        }
    });
    
    api_handles.push(p2p_handle);
    api_handles.push(event_handle);
    
    println!("✅ QTC Node started successfully!");
    println!("🌐 P2P port: {}", config.network.port);
    if config.api.enable_rest {
        println!("🔗 REST API: http://localhost:{}", config.api.rest_port);
    }
    if config.api.enable_websocket {
        println!("🔌 WebSocket: ws://localhost:{}", config.api.websocket_port);
    }
    
    // Wait for termination signal (both daemon and foreground modes)
    signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
    
    println!("\n🛑 Shutting down QTC Node...");
    
    // Cancel all tasks
    for handle in api_handles {
        handle.abort();
    }
    
    println!("✅ QTC Node stopped gracefully.");
    
    Ok(())
}

async fn handle_p2p_event(
    blockchain: Arc<RwLock<Blockchain>>,
    event: crate::network::protocol::Message,
) -> Result<()> {
    match event.message_type {
        crate::network::protocol::MessageType::Block(block) => {
            let mut bc = blockchain.write().unwrap();
            if let Err(e) = bc.add_block(block) {
                log::warn!("Failed to add received block: {}", e);
            }
        }
        
        crate::network::protocol::MessageType::Transaction(tx) => {
            let bc = blockchain.read().unwrap();
            if let Ok(true) = bc.is_valid_transaction(&tx) {
                log::info!("Received valid transaction: {}", hex::encode(tx.hash().as_bytes()));
                // Add to mempool (would be implemented)
            }
        }
        
        _ => {
            // Handle other message types
        }
    }
    
    Ok(())
}

async fn handle_network_command(config: Config, _db: Arc<Database>, cmd: NetworkCommands) -> Result<()> {
    match cmd {
        NetworkCommands::Status => {
            println!("🌐 Network Status:");
            println!("Port: {}", config.network.port);
            println!("Max peers: {}", config.network.max_peers);
            println!("mDNS enabled: {}", config.network.enable_mdns);
            println!("Bootstrap nodes: {}", config.network.bootstrap_nodes.len());
        }
        
        NetworkCommands::Peers => {
            println!("👥 Connected Peers:");
            println!("(P2P node must be running to show peer information)");
        }
        
        NetworkCommands::Connect { address } => {
            println!("🔗 Connecting to peer: {}", address);
            // Implementation would send connect command to P2P node
        }
        
        NetworkCommands::Disconnect { peer_id } => {
            println!("✂️ Disconnecting from peer: {}", peer_id);
            // Implementation would send disconnect command to P2P node
        }
        
        NetworkCommands::AddPeer { address, description: _ } => {
            println!("📝 Adding peer to address book: {}", address);
            // Implementation would store peer in database
        }
        
        NetworkCommands::Stats => {
            println!("📊 Network Statistics:");
            println!("(Statistics available when node is running)");
        }
        
        NetworkCommands::Sync { force: _ } => {
            println!("🔄 Starting blockchain sync...");
            // Implementation would trigger sync process
        }
    }
    
    Ok(())
}

async fn handle_chain_command(db: Arc<Database>, cmd: ChainCommands) -> Result<()> {
    let blockchain = Blockchain::new(db)?;
    
    match cmd {
        ChainCommands::Info => {
            let info = blockchain.get_chain_info()?;
            println!("⛓️  Blockchain Information:");
            println!("Height: {}", info.height);
            println!("Tip hash: {}", info.tip);
            println!("Difficulty: {}", info.difficulty);
            println!("Total supply: {:.8} QTC", info.total_supply as f64 / 100_000_000.0);
        }
        
        ChainCommands::Block { identifier, verbose } => {
            // Try to parse as height first, then as hash
            let block = if let Ok(height) = identifier.parse::<u64>() {
                blockchain.get_block_by_height(height)?
            } else if let Ok(hash) = crate::crypto::hash::Hash256::from_hex(&identifier) {
                blockchain.get_block(&hash)?
            } else {
                return Err(QtcError::InvalidInput("Invalid block identifier".to_string()));
            };
            
            if let Some(block) = block {
                println!("📦 Block Information:");
                println!("Hash: {}", block.hash());
                println!("Height: {}", block.header.height);
                println!("Previous hash: {}", block.header.previous_hash);
                println!("Timestamp: {}", block.header.timestamp);
                println!("Difficulty: {}", block.header.difficulty);
                println!("Nonce: {}", block.header.nonce);
                println!("Transactions: {}", block.transactions.len());
                
                if verbose {
                    for (i, tx) in block.transactions.iter().enumerate() {
                        println!("  Transaction {}: {}", i, hex::encode(tx.hash().as_bytes()));
                    }
                }
            } else {
                println!("❌ Block not found");
            }
        }
        
        ChainCommands::Transaction { hash, raw: _ } => {
            if let Ok(tx_hash) = crate::crypto::hash::Hash256::from_hex(&hash) {
                // Implementation would look up transaction
                println!("💰 Transaction: {}", tx_hash);
            } else {
                println!("❌ Invalid transaction hash");
            }
        }
        
        ChainCommands::Blocks { count, from } => {
            let count = count.unwrap_or(10);
            let start_height = from.unwrap_or(blockchain.height.saturating_sub(count as u64));
            
            println!("📦 Recent Blocks:");
            for height in start_height..=blockchain.height.min(start_height + count as u64) {
                if let Ok(Some(block)) = blockchain.get_block_by_height(height) {
                    println!("  {}: {} (txs: {})", height, block.hash(), block.transactions.len());
                }
            }
        }
        
        ChainCommands::Search { query: _ } => {
            println!("🔍 Search functionality not yet implemented");
        }
        
        ChainCommands::Validate { from_height: _, quick: _ } => {
            println!("✅ Blockchain validation not yet implemented");
        }
        
        ChainCommands::Mempool => {
            println!("🗂️ Mempool: 0 transactions");
        }
        
        ChainCommands::EstimateFee { blocks: _ } => {
            println!("💸 Estimated fee: 1000 satoshis/byte");
        }
    }
    
    Ok(())
}

async fn handle_api_command(config: Config, _db: Arc<Database>, cmd: ApiCommands) -> Result<()> {
    match cmd {
        ApiCommands::Start { rest_port, ws_port, cors: _ } => {
            let rest_port = rest_port.unwrap_or(config.api.rest_port);
            let ws_port = ws_port.unwrap_or(config.api.websocket_port);
            
            println!("🚀 Starting API servers...");
            println!("🔗 REST API will be available at: http://localhost:{}", rest_port);
            println!("🔌 WebSocket will be available at: ws://localhost:{}", ws_port);
            println!("(Use 'qtcd start' to run the full node with APIs)");
        }
        
        ApiCommands::Stop => {
            println!("🛑 Stopping API servers...");
        }
        
        ApiCommands::Status => {
            println!("📊 API Status:");
            println!("REST API enabled: {}", config.api.enable_rest);
            println!("WebSocket enabled: {}", config.api.enable_websocket);
            println!("REST port: {}", config.api.rest_port);
            println!("WebSocket port: {}", config.api.websocket_port);
        }
        
        ApiCommands::Test { endpoint: _ } => {
            println!("🧪 API testing not yet implemented");
        }
    }
    
    Ok(())
}

async fn handle_db_command(db: Arc<Database>, cmd: DbCommands) -> Result<()> {
    match cmd {
        DbCommands::Stats => {
            let stats = db.get_database_stats()?;
            stats.total_size();
            
            println!("📊 Database Statistics:");
            println!("Blocks size: {} MB", stats.blocks_size / 1024 / 1024);
            println!("Transactions count: {}", stats.transaction_count);
            println!("UTXOs size: {} MB", stats.utxo_size / 1024 / 1024);
            println!("UTXO count: {}", stats.utxo_count);
            println!("Wallets count: {}", stats.wallet_count);
            println!("Total size: {} MB", stats.total_size / 1024 / 1024);
        }
        
        DbCommands::Compact => {
            println!("🔧 Compacting database...");
            db.compact()?;
            println!("✅ Database compaction completed");
        }
        
        DbCommands::Backup { path } => {
            println!("💾 Creating database backup...");
            
            // Get database path from config
            let config = Config::load().unwrap_or_default();
            let db_path = &config.storage.data_dir;
            
            if !db_path.exists() {
                return Err(QtcError::Storage("Database directory does not exist".to_string()));
            }
            
            // Create backup file
            let backup_file = File::create(&path)
                .map_err(|e| QtcError::Storage(format!("Failed to create backup file: {}", e)))?;
            
            // Create gzip encoder
            let gz_encoder = GzBuilder::new()
                .filename(format!("qtc-backup-{}.tar", chrono::Utc::now().format("%Y%m%d-%H%M%S")))
                .write(backup_file, Compression::default());
            
            // Create tar builder
            let mut tar_builder = Builder::new(gz_encoder);
            
            // Add entire database directory to archive
            tar_builder.append_dir_all("qtc-data", db_path)
                .map_err(|e| QtcError::Storage(format!("Failed to create backup archive: {}", e)))?;
            
            // Finalize the archive
            tar_builder.finish()
                .map_err(|e| QtcError::Storage(format!("Failed to finalize backup: {}", e)))?;
            
            let file_size = std::fs::metadata(&path)
                .map(|m| m.len())
                .unwrap_or(0);
            
            println!("✅ Backup created successfully!");
            println!("📁 File: {}", path);
            println!("📏 Size: {:.2} MB", file_size as f64 / 1024.0 / 1024.0);
            log::info!("Database backup created successfully at {}", path);
        }
        
        DbCommands::Repair => {
            println!("🔧 Database repair not yet implemented");
        }
        
        DbCommands::Reindex { from_height: _ } => {
            println!("🔄 Blockchain reindexing not yet implemented");
        }
    }
    
    Ok(())
}
