use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DifficultyTarget {
    pub difficulty: u32,
    pub target_bits: u32,
    pub target_hash: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct DifficultyCalculator {
    pub target_block_time: u64, // seconds
    pub adjustment_interval: u64, // blocks
    pub max_adjustment_factor: f64,
    pub min_difficulty: u32,
    pub max_difficulty: u32,
}

impl DifficultyCalculator {
    pub fn new() -> Self {
        Self {
            target_block_time: 450, // 7.5 minutes
            adjustment_interval: 10, // Adjust every 10 blocks
            max_adjustment_factor: 4.0, // Max 4x adjustment per period
            min_difficulty: 6, // Very easy minimum difficulty for testing
            max_difficulty: 255, // Theoretical maximum
        }
    }
    
    pub fn with_params(
        target_block_time: u64,
        adjustment_interval: u64,
        max_adjustment_factor: f64,
    ) -> Self {
        Self {
            target_block_time,
            adjustment_interval,
            max_adjustment_factor,
            min_difficulty: 6, // Very easy minimum difficulty for testing
            max_difficulty: 255,
        }
    }
    
    pub fn calculate_next_difficulty(
        &self,
        current_difficulty: u32,
        block_times: &[u64],
    ) -> Result<u32> {
        if block_times.len() < 2 {
            return Ok(current_difficulty);
        }
        
        // Calculate actual time taken for the period
        let actual_time = self.calculate_actual_time(block_times)?;
        let expected_time = self.target_block_time * (block_times.len() - 1) as u64;
        
        if expected_time == 0 {
            return Ok(current_difficulty);
        }
        
        // Calculate adjustment ratio
        let time_ratio = expected_time as f64 / actual_time as f64;
        
        // Apply limits to prevent wild swings
        let limited_ratio = time_ratio
            .max(1.0 / self.max_adjustment_factor)
            .min(self.max_adjustment_factor);
        
        // Calculate new difficulty
        let new_difficulty_f64 = current_difficulty as f64 * limited_ratio;
        let new_difficulty = new_difficulty_f64.round() as u32;
        
        // Apply absolute bounds
        let bounded_difficulty = new_difficulty
            .max(self.min_difficulty)
            .min(self.max_difficulty);
        
        log::debug!(
            "Difficulty adjustment: {} -> {} (ratio: {:.3}, actual time: {}s, expected: {}s)",
            current_difficulty,
            bounded_difficulty,
            limited_ratio,
            actual_time,
            expected_time
        );
        
        Ok(bounded_difficulty)
    }
    
    fn calculate_actual_time(&self, block_times: &[u64]) -> Result<u64> {
        if block_times.len() < 2 {
            return Err(QtcError::Consensus("Not enough block times".to_string()));
        }
        
        let first_time = block_times[0];
        let last_time = block_times[block_times.len() - 1];
        
        if last_time < first_time {
            return Err(QtcError::Consensus("Invalid block times order".to_string()));
        }
        
        Ok(last_time - first_time)
    }
    
    pub fn difficulty_to_target(&self, difficulty: u32) -> DifficultyTarget {
        // Convert difficulty to target hash
        let mut target_hash = [0xFFu8; 32];
        
        // Set leading zeros based on difficulty
        let zero_bytes = difficulty / 8;
        let remaining_bits = difficulty % 8;
        
        // Set full zero bytes
        for i in 0..(zero_bytes as usize).min(32) {
            target_hash[i] = 0x00;
        }
        
        // Set partial zero bits
        if zero_bytes < 32 && remaining_bits > 0 {
            let mask = 0xFF >> remaining_bits;
            target_hash[zero_bytes as usize] = mask;
        }
        
        DifficultyTarget {
            difficulty,
            target_bits: self.difficulty_to_bits(difficulty),
            target_hash,
        }
    }
    
    fn difficulty_to_bits(&self, difficulty: u32) -> u32 {
        // Simplified conversion to compact target representation
        // Similar to Bitcoin's nBits format but adapted for our use
        
        if difficulty == 0 {
            return 0x207FFFFF; // Maximum target
        }
        
        // Calculate the compact representation
        let _leading_zeros = difficulty / 8;
        let shift = 256 - difficulty;
        
        if shift >= 256 {
            return 0x00000000;
        }
        
        // This is a simplified implementation
        // Production would use proper compact target format
        0x1D00FFFF >> (difficulty / 4).min(24)
    }
    
    pub fn bits_to_difficulty(&self, bits: u32) -> u32 {
        // Convert compact target representation back to difficulty
        // Inverse of difficulty_to_bits
        
        if bits == 0 {
            return self.max_difficulty;
        }
        
        // Simplified implementation
        let shift = 24 - (bits.leading_zeros().min(24));
        shift * 4
    }
    
    pub fn estimate_hashrate(&self, difficulty: u32, block_time: u64) -> f64 {
        if block_time == 0 {
            return 0.0;
        }
        
        // Estimate network hashrate based on difficulty and actual block time
        let target_hashes = 2_u64.pow(difficulty.min(32)) as f64;
        target_hashes / block_time as f64
    }
    
    pub fn time_to_next_adjustment(&self, current_height: u64) -> u64 {
        let blocks_since_adjustment = current_height % self.adjustment_interval;
        self.adjustment_interval - blocks_since_adjustment
    }
    
    pub fn validate_difficulty(&self, difficulty: u32) -> Result<()> {
        if difficulty < self.min_difficulty {
            return Err(QtcError::Consensus(format!(
                "Difficulty {} below minimum {}",
                difficulty, self.min_difficulty
            )));
        }
        
        if difficulty > self.max_difficulty {
            return Err(QtcError::Consensus(format!(
                "Difficulty {} above maximum {}",
                difficulty, self.max_difficulty
            )));
        }
        
        Ok(())
    }
    
    pub fn should_adjust_difficulty(&self, height: u64) -> bool {
        height > 0 && height % self.adjustment_interval == 0
    }
    
    pub fn calculate_work(&self, difficulty: u32) -> u128 {
        // Calculate cumulative work for a given difficulty
        // Higher difficulty = more work
        
        if difficulty == 0 {
            return 0;
        }
        
        // Simplified work calculation
        // In production, would use proper work calculation like Bitcoin
        2_u128.pow(difficulty.min(127))
    }
    
    pub fn get_adjustment_params(&self) -> (u64, u64, f64) {
        (
            self.target_block_time,
            self.adjustment_interval,
            self.max_adjustment_factor,
        )
    }
}

impl Default for DifficultyCalculator {
    fn default() -> Self {
        Self::new()
    }
}

// Utility functions for difficulty analysis
pub struct DifficultyAnalyzer;

impl DifficultyAnalyzer {
    pub fn analyze_difficulty_trend(difficulties: &[u32]) -> DifficultyTrend {
        if difficulties.len() < 2 {
            return DifficultyTrend::Stable;
        }
        
        let recent = &difficulties[difficulties.len().saturating_sub(10)..];
        let first = recent[0] as f64;
        let last = recent[recent.len() - 1] as f64;
        
        let change_ratio = last / first;
        
        if change_ratio > 1.2 {
            DifficultyTrend::Increasing
        } else if change_ratio < 0.8 {
            DifficultyTrend::Decreasing
        } else {
            DifficultyTrend::Stable
        }
    }
    
    pub fn predict_next_difficulty(
        calculator: &DifficultyCalculator,
        recent_times: &[u64],
        current_difficulty: u32,
    ) -> Result<u32> {
        calculator.calculate_next_difficulty(current_difficulty, recent_times)
    }
    
    pub fn calculate_mining_profitability(
        difficulty: u32,
        hashrate: f64,
        power_cost_per_kwh: f64,
        power_consumption_watts: f64,
        qtc_price: f64,
        block_reward: u64,
    ) -> MiningProfitability {
        let target_block_time = 450.0; // 7.5 minutes
        let blocks_per_day = 24.0 * 60.0 * 60.0 / target_block_time;
        
        // Estimate blocks mined per day
        let network_hashrate = 2_f64.powi(difficulty as i32) / target_block_time;
        let hash_share = hashrate / network_hashrate;
        let blocks_per_day_mined = blocks_per_day * hash_share;
        
        // Calculate revenue
        let qtc_per_day = blocks_per_day_mined * (block_reward as f64 / 100_000_000.0); // Convert satoshis
        let revenue_per_day = qtc_per_day * qtc_price;
        
        // Calculate costs
        let power_per_day_kwh = power_consumption_watts * 24.0 / 1000.0;
        let cost_per_day = power_per_day_kwh * power_cost_per_kwh;
        
        // Calculate profit
        let profit_per_day = revenue_per_day - cost_per_day;
        
        MiningProfitability {
            daily_revenue: revenue_per_day,
            daily_cost: cost_per_day,
            daily_profit: profit_per_day,
            blocks_per_day: blocks_per_day_mined,
            qtc_per_day,
            profitable: profit_per_day > 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum DifficultyTrend {
    Increasing,
    Decreasing,
    Stable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningProfitability {
    pub daily_revenue: f64,
    pub daily_cost: f64,
    pub daily_profit: f64,
    pub blocks_per_day: f64,
    pub qtc_per_day: f64,
    pub profitable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_difficulty_calculation() {
        let calculator = DifficultyCalculator::new();
        
        // Test with blocks that took too long (difficulty should decrease)
        let slow_times = vec![0, 600, 1200, 1800]; // 10-minute blocks instead of 7.5
        let new_difficulty = calculator.calculate_next_difficulty(8, &slow_times).unwrap();
        assert!(new_difficulty < 8);
        
        // Test with blocks that were too fast (difficulty should increase)
        let fast_times = vec![0, 300, 600, 900]; // 5-minute blocks instead of 7.5
        let new_difficulty = calculator.calculate_next_difficulty(8, &fast_times).unwrap();
        assert!(new_difficulty > 8);
    }
    
    #[test]
    fn test_difficulty_bounds() {
        let calculator = DifficultyCalculator::new();
        
        // Test minimum bound
        let very_slow_times = vec![0, 10000, 20000, 30000]; // Very slow blocks
        let new_difficulty = calculator.calculate_next_difficulty(1, &very_slow_times).unwrap();
        assert_eq!(new_difficulty, calculator.min_difficulty);
        
        // Test maximum adjustment factor
        let extremely_fast_times = vec![0, 1, 2, 3]; // Extremely fast blocks
        let new_difficulty = calculator.calculate_next_difficulty(8, &extremely_fast_times).unwrap();
        assert!(new_difficulty <= 8 * calculator.max_adjustment_factor as u32);
    }
    
    #[test]
    fn test_difficulty_to_target() {
        let calculator = DifficultyCalculator::new();
        
        let target = calculator.difficulty_to_target(16); // 2 zero bytes
        assert_eq!(target.difficulty, 16);
        assert_eq!(target.target_hash[0], 0x00);
        assert_eq!(target.target_hash[1], 0x00);
        assert_ne!(target.target_hash[2], 0x00);
    }
    
    #[test]
    fn test_hashrate_estimation() {
        let calculator = DifficultyCalculator::new();
        
        let hashrate = calculator.estimate_hashrate(16, 450); // Target block time
        assert!(hashrate > 0.0);
        
        // Faster block should indicate higher hashrate
        let faster_hashrate = calculator.estimate_hashrate(16, 225);
        assert!(faster_hashrate > hashrate);
    }
    
    #[test]
    fn test_difficulty_trend() {
        let increasing = vec![4, 5, 6, 7, 8, 9, 10];
        assert_eq!(DifficultyAnalyzer::analyze_difficulty_trend(&increasing), DifficultyTrend::Increasing);
        
        let decreasing = vec![10, 9, 8, 7, 6, 5, 4];
        assert_eq!(DifficultyAnalyzer::analyze_difficulty_trend(&decreasing), DifficultyTrend::Decreasing);
        
        let stable = vec![8, 8, 9, 8, 8, 9, 8];
        assert_eq!(DifficultyAnalyzer::analyze_difficulty_trend(&stable), DifficultyTrend::Stable);
    }
}
