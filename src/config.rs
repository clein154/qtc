use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkType {
    Mainnet,
    Testnet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub network_type: NetworkType,
    pub network: NetworkConfig,
    pub mining: MiningConfig,
    pub storage: StorageConfig,
    pub api: ApiConfig,
    pub consensus: ConsensusConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub port: u16,
    pub max_peers: usize,
    pub bootstrap_nodes: Vec<String>,
    pub enable_mdns: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningConfig {
    pub threads: usize,
    pub target_block_time: u64, // seconds
    pub difficulty_adjustment_blocks: u64,
    pub initial_difficulty: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub data_dir: PathBuf,
    pub max_db_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub enable_rest: bool,
    pub rest_port: u16,
    pub enable_websocket: bool,
    pub websocket_port: u16,
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub max_block_size: usize,
    pub min_transaction_fee: u64,
    pub coinbase_reward: u64,
    pub halving_interval: u64, // blocks
    pub max_supply: u64,
}

impl Default for Config {
    fn default() -> Self {
        let home_dir = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let data_dir = PathBuf::from(home_dir).join(".qtc");
        
        Self {
            network_type: NetworkType::Mainnet,
            network: NetworkConfig {
                port: 8333,
                max_peers: 50,
                bootstrap_nodes: vec![],
                enable_mdns: true,
            },
            mining: MiningConfig {
                threads: num_cpus::get(),
                target_block_time: 450, // 7.5 minutes
                difficulty_adjustment_blocks: 10,
                initial_difficulty: 20, // Higher initial difficulty to prevent millisecond blocks
            },
            storage: StorageConfig {
                data_dir,
                max_db_size: 1024 * 1024 * 1024, // 1GB
            },
            api: ApiConfig {
                enable_rest: true,
                rest_port: 8000,
                enable_websocket: true,
                websocket_port: 8001,
                cors_origins: vec!["*".to_string()],
            },
            consensus: ConsensusConfig {
                max_block_size: 1024 * 1024, // 1MB
                min_transaction_fee: 1000, // 0.00001 QTC
                coinbase_reward: 2710000000, // 27.1 QTC in satoshis
                halving_interval: 262800, // 5 years at 7.5 min blocks
                max_supply: 1999999900000000, // 19,999,999 QTC in satoshis
            },
        }
    }
}

impl Config {
    pub fn testnet() -> Self {
        let home_dir = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let data_dir = PathBuf::from(home_dir).join(".qtc-testnet");
        
        Self {
            network_type: NetworkType::Testnet,
            network: NetworkConfig {
                port: 18333, // Different port for testnet
                max_peers: 20,
                bootstrap_nodes: vec![],
                enable_mdns: true,
            },
            mining: MiningConfig {
                threads: num_cpus::get(),
                target_block_time: 450, // Same target time
                difficulty_adjustment_blocks: 10,
                initial_difficulty: 16, // Lower difficulty for testing
            },
            storage: StorageConfig {
                data_dir,
                max_db_size: 256 * 1024 * 1024, // 256MB for testnet
            },
            api: ApiConfig {
                enable_rest: true,
                rest_port: 18080, // Different API port
                enable_websocket: true,
                websocket_port: 18081,
                cors_origins: vec!["*".to_string()],
            },
            consensus: ConsensusConfig {
                max_block_size: 1024 * 1024, // 1MB
                min_transaction_fee: 100, // Lower fee for testing
                coinbase_reward: 2710000000, // Same reward structure
                halving_interval: 262800,
                max_supply: 1999999900000000,
            },
        }
    }
    
    pub fn is_testnet(&self) -> bool {
        self.network_type == NetworkType::Testnet
    }
    
    pub fn get_genesis_message(&self) -> String {
        match self.network_type {
            NetworkType::Mainnet => "The Times 10/Jul/2025 Chancellor on brink of second bailout for banks - QTC Genesis".to_string(),
            NetworkType::Testnet => "QTC Testnet Genesis - Jul 2025 - Testing blockchain implementation".to_string(),
        }
    }
    
    pub fn get_genesis_address(&self) -> String {
        match self.network_type {
            NetworkType::Mainnet => "qtc1qw508d6qejxtdg4y5r3zarvary0c5xw7kxdz6v9".to_string(),
            NetworkType::Testnet => "qtctestnet1qw508d6qejxtdg4y5r3zarvary0c5xw7k2pz4m5".to_string(),
        }
    }

    pub fn load() -> anyhow::Result<Self> {
        let config_path = Self::config_path();
        
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = serde_json::from_str(&content)?;
            Ok(config)
        } else {
            let config = Self::default();
            config.save()?;
            Ok(config)
        }
    }
    
    pub fn save(&self) -> anyhow::Result<()> {
        let config_path = Self::config_path();
        
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, content)?;
        
        Ok(())
    }
    
    fn config_path() -> PathBuf {
        let home_dir = env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home_dir).join(".qtc").join("config.json")
    }
}

// Add num_cpus dependency to Cargo.toml
