use crate::core::{Block, Blockchain};
use crate::mining::randomx::{RandomXMiner, RandomXHash};
use crate::mining::difficulty::DifficultyCalculator;
use crate::crypto::hash::Hash256;
use crate::{QtcError, Result};
use std::sync::{Arc, RwLock, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningStats {
    pub is_mining: bool,
    pub hashrate: f64,
    pub total_hashes: u64,
    pub blocks_mined: u64,
    pub last_block_time: Option<u64>,
    pub current_difficulty: u32,
    pub mining_address: String,
    pub threads: usize,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct MiningResult {
    pub block: Block,
    pub nonce: u64,
    pub hash: Hash256,
    pub target_met: bool,
}

pub struct Miner {
    blockchain: Arc<RwLock<Blockchain>>,
    randomx_miner: Arc<RandomXMiner>,
    difficulty_calc: DifficultyCalculator,
    mining_address: String,
    is_mining: Arc<AtomicBool>,
    stats: Arc<RwLock<MiningStats>>,
    hash_counter: Arc<AtomicU64>,
    blocks_mined: Arc<AtomicU64>,
    start_time: Instant,
    threads: usize,
}

impl Miner {
    pub fn new(
        blockchain: Arc<RwLock<Blockchain>>,
        mining_address: String,
        threads: usize,
    ) -> Result<Self> {
        // Validate mining address
        if !crate::crypto::keys::is_valid_address(&mining_address) {
            return Err(QtcError::Mining("Invalid mining address".to_string()));
        }
        
        // Initialize RandomX with current blockchain tip as seed
        let seed = {
            let bc = blockchain.read().unwrap();
            bc.tip.as_bytes().to_vec()
        };
        
        let randomx_miner = Arc::new(RandomXMiner::new(&seed, true)?);
        let difficulty_calc = DifficultyCalculator::new();
        
        let stats = MiningStats {
            is_mining: false,
            hashrate: 0.0,
            total_hashes: 0,
            blocks_mined: 0,
            last_block_time: None,
            current_difficulty: 4,
            mining_address: mining_address.clone(),
            threads,
            uptime_seconds: 0,
        };
        
        Ok(Self {
            blockchain,
            randomx_miner,
            difficulty_calc,
            mining_address,
            is_mining: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RwLock::new(stats)),
            hash_counter: Arc::new(AtomicU64::new(0)),
            blocks_mined: Arc::new(AtomicU64::new(0)),
            start_time: Instant::now(),
            threads,
        })
    }
    
    pub async fn start_mining(&self) -> Result<()> {
        if self.is_mining.load(Ordering::Relaxed) {
            return Err(QtcError::Mining("Mining already started".to_string()));
        }
        
        log::info!("🚀 Starting QTC mining with {} threads", self.threads);
        log::info!("⛏️  Mining to address: {}", self.mining_address);
        
        self.is_mining.store(true, Ordering::Relaxed);
        
        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.is_mining = true;
        }
        
        // Start mining threads
        let mut handles = Vec::new();
        
        for thread_id in 0..self.threads {
            let handle = self.spawn_mining_thread(thread_id).await?;
            handles.push(handle);
        }
        
        // Start stats updating task
        let stats_handle = self.spawn_stats_updater().await;
        handles.push(stats_handle);
        
        // Wait for all threads
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Mining thread error: {}", e);
            }
        }
        
        Ok(())
    }
    
    pub fn stop_mining(&self) {
        log::info!("🛑 Stopping QTC mining");
        self.is_mining.store(false, Ordering::Relaxed);
        
        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.is_mining = false;
        }
    }
    
    pub fn is_mining(&self) -> bool {
        self.is_mining.load(Ordering::Relaxed)
    }
    
    pub fn get_stats(&self) -> MiningStats {
        let stats = self.stats.read().unwrap();
        let mut stats_copy = stats.clone();
        
        // Update runtime stats
        stats_copy.total_hashes = self.hash_counter.load(Ordering::Relaxed);
        stats_copy.blocks_mined = self.blocks_mined.load(Ordering::Relaxed);
        stats_copy.uptime_seconds = self.start_time.elapsed().as_secs();
        
        stats_copy
    }
    
    async fn spawn_mining_thread(&self, thread_id: usize) -> Result<tokio::task::JoinHandle<()>> {
        let blockchain = self.blockchain.clone();
        let mining_address = self.mining_address.clone();
        let is_mining = self.is_mining.clone();
        let hash_counter = self.hash_counter.clone();
        let blocks_mined = self.blocks_mined.clone();
        let stats = self.stats.clone();
        
        // Create RandomX miner for this thread
        let seed = {
            let bc = blockchain.read().unwrap();
            bc.tip.as_bytes().to_vec()
        };
        let thread_miner = RandomXMiner::new(&seed, false)?; // Light mode for worker threads
        
        let handle = tokio::spawn(async move {
            log::info!("⛏️  Mining thread {} started", thread_id);
            
            let mut nonce_start = thread_id as u64 * 1000000; // Spread nonce ranges
            
            while is_mining.load(Ordering::Relaxed) {
                match Self::mine_single_attempt(
                    &blockchain,
                    &thread_miner,
                    &mining_address,
                    nonce_start,
                    &hash_counter,
                ).await {
                    Ok(Some(result)) => {
                        log::info!("🎉 Block mined by thread {}! Hash: {}", thread_id, result.hash);
                        
                        // Add block to blockchain
                        {
                            let mut bc = blockchain.write().unwrap();
                            if let Err(e) = bc.add_block(result.block) {
                                log::error!("Failed to add mined block: {}", e);
                            } else {
                                blocks_mined.fetch_add(1, Ordering::Relaxed);
                                
                                // Update stats
                                {
                                    let mut stats = stats.write().unwrap();
                                    stats.last_block_time = Some(chrono::Utc::now().timestamp() as u64);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // No block found, continue mining
                    }
                    Err(e) => {
                        log::error!("Mining error in thread {}: {}", thread_id, e);
                        sleep(Duration::from_millis(100)).await;
                    }
                }
                
                nonce_start += 1000; // Move to next nonce range
                
                // Small delay to prevent overwhelming the system
                if nonce_start % 10000 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            
            log::info!("⛏️  Mining thread {} stopped", thread_id);
        });
        
        Ok(handle)
    }
    
    async fn mine_single_attempt(
        blockchain: &Arc<RwLock<Blockchain>>,
        miner: &RandomXMiner,
        mining_address: &str,
        nonce_start: u64,
        hash_counter: &Arc<AtomicU64>,
    ) -> Result<Option<MiningResult>> {
        // Get current blockchain state
        let (mut block, difficulty) = {
            let bc = blockchain.read().unwrap();
            let height = bc.height + 1;
            let difficulty = bc.get_current_difficulty()?;
            
            // Create coinbase transaction
            let reward = crate::consensus::monetary::MonetaryPolicy::new().coinbase_reward(height);
            let coinbase_tx = crate::core::Transaction::new_coinbase(
                mining_address.to_string(),
                reward,
                format!("QTC Block {} mined by thread", height),
            );
            
            let block = Block::new(
                bc.tip,
                vec![coinbase_tx],
                difficulty,
                height,
            );
            
            (block, difficulty)
        };
        
        // Try mining with different nonces
        for nonce_offset in 0..1000 {
            let nonce = nonce_start + nonce_offset;
            block.set_nonce(nonce);
            
            // Create block header data for hashing
            let header_data = bincode::serialize(&block.header)
                .map_err(|e| QtcError::Mining(format!("Failed to serialize block header: {}", e)))?;
            
            // Hash with RandomX
            let randomx_hash = miner.hash(&header_data)?;
            hash_counter.fetch_add(1, Ordering::Relaxed);
            
            // Convert RandomX hash to our Hash256 format
            let block_hash = Hash256::new(*randomx_hash.as_bytes());
            
            // Check if it meets difficulty
            if randomx_hash.meets_difficulty(difficulty) {
                return Ok(Some(MiningResult {
                    block,
                    nonce,
                    hash: block_hash,
                    target_met: true,
                }));
            }
        }
        
        Ok(None)
    }
    
    async fn spawn_stats_updater(&self) -> tokio::task::JoinHandle<()> {
        let is_mining = self.is_mining.clone();
        let hash_counter = self.hash_counter.clone();
        let stats = self.stats.clone();
        let blockchain = self.blockchain.clone();
        
        tokio::spawn(async move {
            let mut last_hashes = 0u64;
            let mut last_time = Instant::now();
            
            while is_mining.load(Ordering::Relaxed) {
                sleep(Duration::from_secs(5)).await;
                
                let current_hashes = hash_counter.load(Ordering::Relaxed);
                let current_time = Instant::now();
                let elapsed = current_time.duration_since(last_time).as_secs_f64();
                
                if elapsed > 0.0 {
                    let hashrate = (current_hashes - last_hashes) as f64 / elapsed;
                    
                    // Update stats
                    {
                        let mut stats = stats.write().unwrap();
                        stats.hashrate = hashrate;
                        
                        // Update current difficulty
                        if let Ok(bc) = blockchain.read() {
                            if let Ok(difficulty) = bc.get_current_difficulty() {
                                stats.current_difficulty = difficulty;
                            }
                        }
                    }
                    
                    log::info!("⛏️  Current hashrate: {:.2} H/s", hashrate);
                }
                
                last_hashes = current_hashes;
                last_time = current_time;
            }
        })
    }
    
    pub async fn mine_single_block(&self) -> Result<Option<Block>> {
        if self.is_mining.load(Ordering::Relaxed) {
            return Err(QtcError::Mining("Cannot mine single block while continuous mining is active".to_string()));
        }
        
        log::info!("⛏️  Mining single block...");
        
        // Get current blockchain state
        let (mut block, difficulty) = {
            let bc = self.blockchain.read().unwrap();
            let height = bc.height + 1;
            let difficulty = bc.get_current_difficulty()?;
            
            // Create coinbase transaction
            let reward = crate::consensus::monetary::MonetaryPolicy::new().coinbase_reward(height);
            let coinbase_tx = crate::core::Transaction::new_coinbase(
                self.mining_address.clone(),
                reward,
                format!("QTC Block {} - single mine", height),
            );
            
            let block = Block::new(
                bc.tip,
                vec![coinbase_tx],
                difficulty,
                height,
            );
            
            (block, difficulty)
        };
        
        // Mine the block
        let start_time = Instant::now();
        let mut nonce = 0u64;
        
        loop {
            block.set_nonce(nonce);
            
            // Create block header data for hashing
            let header_data = bincode::serialize(&block.header)
                .map_err(|e| QtcError::Mining(format!("Failed to serialize block header: {}", e)))?;
            
            // Hash with RandomX
            let randomx_hash = self.randomx_miner.hash(&header_data)?;
            self.hash_counter.fetch_add(1, Ordering::Relaxed);
            
            // Check if it meets difficulty
            if randomx_hash.meets_difficulty(difficulty) {
                let elapsed = start_time.elapsed();
                let hashrate = nonce as f64 / elapsed.as_secs_f64();
                
                log::info!("✅ Block mined! Nonce: {}, Time: {:.2}s, Hashrate: {:.2} H/s", 
                          nonce, elapsed.as_secs_f64(), hashrate);
                
                return Ok(Some(block));
            }
            
            nonce += 1;
            
            // Check for timeout (optional)
            if start_time.elapsed() > Duration::from_secs(300) { // 5 minutes
                log::warn!("⏱️  Single block mining timeout after 5 minutes");
                return Ok(None);
            }
            
            // Yield control periodically
            if nonce % 1000 == 0 {
                tokio::task::yield_now().await;
            }
        }
    }
    
    pub fn update_mining_address(&mut self, new_address: String) -> Result<()> {
        if !crate::crypto::keys::is_valid_address(&new_address) {
            return Err(QtcError::Mining("Invalid mining address".to_string()));
        }
        
        self.mining_address = new_address.clone();
        
        // Update stats
        {
            let mut stats = self.stats.write().unwrap();
            stats.mining_address = new_address;
        }
        
        log::info!("📍 Mining address updated to: {}", self.mining_address);
        Ok(())
    }
    
    pub fn estimate_time_to_block(&self) -> Option<Duration> {
        let stats = self.get_stats();
        
        if stats.hashrate <= 0.0 {
            return None;
        }
        
        // Rough estimation based on current difficulty and hashrate
        let target_time_seconds = 450.0; // 7.5 minutes target
        let difficulty_factor = 2_u64.pow(stats.current_difficulty);
        let estimated_hashes_needed = difficulty_factor as f64;
        let estimated_seconds = estimated_hashes_needed / stats.hashrate;
        
        Some(Duration::from_secs_f64(estimated_seconds))
    }
    
    pub async fn benchmark(&self, duration: Duration) -> Result<f64> {
        log::info!("🏃 Running RandomX benchmark for {:?}", duration);
        
        let start_time = Instant::now();
        let mut hashes = 0u64;
        let test_data = b"benchmark test data for randomx performance measurement";
        
        while start_time.elapsed() < duration {
            let _ = self.randomx_miner.hash(test_data)?;
            hashes += 1;
            
            if hashes % 100 == 0 {
                tokio::task::yield_now().await;
            }
        }
        
        let elapsed = start_time.elapsed().as_secs_f64();
        let hashrate = hashes as f64 / elapsed;
        
        log::info!("📊 Benchmark result: {:.2} H/s", hashrate);
        Ok(hashrate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::TempDir;
    use std::sync::Arc;
    
    #[tokio::test]
    async fn test_miner_creation() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let blockchain = Arc::new(RwLock::new(Blockchain::new(db)?));
        
        let miner = Miner::new(
            blockchain,
            "qtc1qw508d6qejxtdg4y5r3zarvary0c5xw7kxdz6v9".to_string(),
            1,
        )?;
        
        assert!(!miner.is_mining());
        assert_eq!(miner.threads, 1);
        
        Ok(())
    }
    
    #[tokio::test]
    async fn test_miner_stats() -> Result<()> {
        let temp_dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(temp_dir.path().join("test.db"))?);
        let blockchain = Arc::new(RwLock::new(Blockchain::new(db)?));
        
        let miner = Miner::new(
            blockchain,
            "qtc1qw508d6qejxtdg4y5r3zarvary0c5xw7kxdz6v9".to_string(),
            2,
        )?;
        
        let stats = miner.get_stats();
        assert!(!stats.is_mining);
        assert_eq!(stats.threads, 2);
        assert_eq!(stats.total_hashes, 0);
        
        Ok(())
    }
}
