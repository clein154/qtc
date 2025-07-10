use crate::{QtcError, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonetaryPolicy {
    pub initial_reward: u64,      // Initial block reward in satoshis
    pub halving_interval: u64,    // Blocks between halvings
    pub max_supply: u64,          // Maximum supply in satoshis
    pub min_fee: u64,            // Minimum transaction fee
    pub dust_threshold: u64,      // Minimum output value
    pub coinbase_maturity: u64,   // Blocks before coinbase can be spent
}

impl MonetaryPolicy {
    pub fn new() -> Self {
        Self {
            initial_reward: 2710000000,      // 27.1 QTC
            halving_interval: 262800,        // ~5 years at 7.5 min blocks
            max_supply: 1999999900000000,    // 19,999,999 QTC
            min_fee: 1000,                   // 0.00001 QTC
            dust_threshold: 546,             // 0.00000546 QTC
            coinbase_maturity: 100,          // 100 blocks (~12.5 hours)
        }
    }
    
    /// Calculate the block reward for a given height
    pub fn coinbase_reward(&self, height: u64) -> u64 {
        let halvings = height / self.halving_interval;
        
        // If too many halvings, reward becomes 0
        if halvings >= 64 {
            return 0;
        }
        
        // Calculate reward after halvings
        let reward = self.initial_reward >> halvings;
        
        // Ensure minimum reward (until max supply is reached)
        if reward == 0 && self.total_supply_at_height(height) < self.max_supply {
            1 // 1 satoshi minimum
        } else {
            reward
        }
    }
    
    /// Calculate total supply at a given height
    pub fn total_supply_at_height(&self, height: u64) -> u64 {
        let mut total_supply = 0u64;
        let mut current_reward = self.initial_reward;
        let mut blocks_processed = 0u64;
        
        while blocks_processed < height && current_reward > 0 {
            let blocks_until_halving = self.halving_interval - (blocks_processed % self.halving_interval);
            let blocks_at_this_reward = std::cmp::min(blocks_until_halving, height - blocks_processed);
            
            total_supply = total_supply.saturating_add(
                current_reward.saturating_mul(blocks_at_this_reward)
            );
            
            blocks_processed += blocks_at_this_reward;
            
            // Check if we need to halve
            if blocks_processed % self.halving_interval == 0 && blocks_processed < height {
                current_reward >>= 1; // Halve the reward
            }
        }
        
        // Cap at max supply
        std::cmp::min(total_supply, self.max_supply)
    }
    
    /// Calculate when max supply will be reached
    pub fn max_supply_height(&self) -> u64 {
        let mut height = 0u64;
        let mut total_supply = 0u64;
        let mut current_reward = self.initial_reward;
        
        while total_supply < self.max_supply && current_reward > 0 {
            let blocks_until_halving = self.halving_interval - (height % self.halving_interval);
            let supply_at_current_rate = current_reward.saturating_mul(blocks_until_halving);
            
            if total_supply.saturating_add(supply_at_current_rate) >= self.max_supply {
                let remaining_supply = self.max_supply - total_supply;
                let remaining_blocks = remaining_supply / current_reward;
                return height + remaining_blocks;
            }
            
            total_supply = total_supply.saturating_add(supply_at_current_rate);
            height += blocks_until_halving;
            current_reward >>= 1; // Halve the reward
        }
        
        height
    }
    
    /// Get the current halving epoch for a given height
    pub fn halving_epoch(&self, height: u64) -> u64 {
        height / self.halving_interval
    }
    
    /// Get blocks until next halving
    pub fn blocks_until_next_halving(&self, height: u64) -> u64 {
        self.halving_interval - (height % self.halving_interval)
    }
    
    /// Check if a coinbase reward is valid for the given height
    pub fn is_valid_coinbase_reward(&self, height: u64, reward: u64, total_fees: u64) -> bool {
        let expected_reward = self.coinbase_reward(height);
        let max_allowed = expected_reward + total_fees;
        
        reward <= max_allowed
    }
    
    /// Calculate inflation rate at a given height
    pub fn inflation_rate_at_height(&self, height: u64) -> f64 {
        if height == 0 {
            return 0.0;
        }
        
        let current_supply = self.total_supply_at_height(height) as f64;
        let annual_blocks = (365.25 * 24.0 * 60.0) / 7.5; // Blocks per year at 7.5 min
        let annual_reward = self.coinbase_reward(height) as f64 * annual_blocks;
        
        if current_supply == 0.0 {
            0.0
        } else {
            (annual_reward / current_supply) * 100.0
        }
    }
    
    /// Get fee policy for transaction validation
    pub fn get_fee_policy(&self) -> FeePolicy {
        FeePolicy {
            min_fee: self.min_fee,
            dust_threshold: self.dust_threshold,
            fee_per_byte: 10, // 10 satoshis per byte
            max_fee_multiplier: 1000, // Max fee is 1000x the base fee
        }
    }
    
    /// Calculate the minimum fee for a transaction of given size
    pub fn calculate_min_fee(&self, tx_size: usize) -> u64 {
        let base_fee = self.min_fee;
        let size_fee = (tx_size as u64) * 10; // 10 satoshis per byte
        
        std::cmp::max(base_fee, size_fee)
    }
    
    /// Check if an output value is above dust threshold
    pub fn is_dust(&self, value: u64) -> bool {
        value < self.dust_threshold
    }
    
    /// Get coinbase maturity requirement
    pub fn get_coinbase_maturity(&self) -> u64 {
        self.coinbase_maturity
    }
    
    /// Calculate transaction priority (for mempool ordering)
    pub fn calculate_priority(&self, tx_size: usize, fee: u64, age: u64) -> f64 {
        if tx_size == 0 {
            return 0.0;
        }
        
        let fee_per_byte = fee as f64 / tx_size as f64;
        let age_factor = (age as f64).sqrt(); // Square root of age for diminishing returns
        
        fee_per_byte * age_factor
    }
    
    /// Get economics info for the current state
    pub fn get_economics_info(&self, height: u64) -> EconomicsInfo {
        let current_reward = self.coinbase_reward(height);
        let total_supply = self.total_supply_at_height(height);
        let inflation_rate = self.inflation_rate_at_height(height);
        let halving_epoch = self.halving_epoch(height);
        let blocks_to_halving = self.blocks_until_next_halving(height);
        let max_supply_height = self.max_supply_height();
        
        EconomicsInfo {
            height,
            current_reward,
            total_supply,
            max_supply: self.max_supply,
            inflation_rate,
            halving_epoch,
            blocks_to_halving,
            is_supply_capped: total_supply >= self.max_supply,
            blocks_to_max_supply: if height < max_supply_height {
                Some(max_supply_height - height)
            } else {
                None
            },
        }
    }
    
    /// Validate monetary policy parameters
    pub fn validate(&self) -> Result<()> {
        if self.initial_reward == 0 {
            return Err(QtcError::Consensus("Initial reward cannot be zero".to_string()));
        }
        
        if self.halving_interval == 0 {
            return Err(QtcError::Consensus("Halving interval cannot be zero".to_string()));
        }
        
        if self.max_supply == 0 {
            return Err(QtcError::Consensus("Max supply cannot be zero".to_string()));
        }
        
        if self.dust_threshold == 0 {
            return Err(QtcError::Consensus("Dust threshold cannot be zero".to_string()));
        }
        
        // Check that max supply is achievable
        let theoretical_max = self.calculate_theoretical_max_supply();
        if theoretical_max < self.max_supply {
            return Err(QtcError::Consensus(
                "Max supply exceeds theoretical maximum from halvings".to_string()
            ));
        }
        
        Ok(())
    }
    
    /// Calculate theoretical maximum supply from infinite halvings
    fn calculate_theoretical_max_supply(&self) -> u64 {
        // Sum of geometric series: initial_reward * halving_interval * 2
        // This is the maximum possible supply from infinite halvings
        self.initial_reward.saturating_mul(self.halving_interval).saturating_mul(2)
    }
}

impl Default for MonetaryPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeePolicy {
    pub min_fee: u64,
    pub dust_threshold: u64,
    pub fee_per_byte: u64,
    pub max_fee_multiplier: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicsInfo {
    pub height: u64,
    pub current_reward: u64,
    pub total_supply: u64,
    pub max_supply: u64,
    pub inflation_rate: f64,
    pub halving_epoch: u64,
    pub blocks_to_halving: u64,
    pub is_supply_capped: bool,
    pub blocks_to_max_supply: Option<u64>,
}

/// Utility functions for monetary calculations
pub struct MonetaryUtils;

impl MonetaryUtils {
    /// Convert satoshis to QTC
    pub fn satoshis_to_qtc(satoshis: u64) -> f64 {
        satoshis as f64 / 100_000_000.0
    }
    
    /// Convert QTC to satoshis
    pub fn qtc_to_satoshis(qtc: f64) -> u64 {
        (qtc * 100_000_000.0) as u64
    }
    
    /// Format QTC amount with proper decimal places
    pub fn format_qtc(satoshis: u64) -> String {
        format!("{:.8}", Self::satoshis_to_qtc(satoshis))
    }
    
    /// Parse QTC string to satoshis
    pub fn parse_qtc(qtc_str: &str) -> Result<u64> {
        let qtc: f64 = qtc_str.parse()
            .map_err(|_| QtcError::InvalidInput("Invalid QTC amount".to_string()))?;
        
        if qtc < 0.0 {
            return Err(QtcError::InvalidInput("QTC amount cannot be negative".to_string()));
        }
        
        if qtc > 21_000_000.0 {
            return Err(QtcError::InvalidInput("QTC amount too large".to_string()));
        }
        
        Ok(Self::qtc_to_satoshis(qtc))
    }
    
    /// Calculate compound growth
    pub fn calculate_compound_growth(principal: f64, rate: f64, periods: f64) -> f64 {
        principal * (1.0 + rate).powf(periods)
    }
    
    /// Calculate present value
    pub fn calculate_present_value(future_value: f64, rate: f64, periods: f64) -> f64 {
        future_value / (1.0 + rate).powf(periods)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_monetary_policy_creation() {
        let policy = MonetaryPolicy::new();
        assert_eq!(policy.initial_reward, 2710000000);
        assert_eq!(policy.halving_interval, 262800);
        assert_eq!(policy.max_supply, 1999999900000000);
    }
    
    #[test]
    fn test_coinbase_reward_calculation() {
        let policy = MonetaryPolicy::new();
        
        // Initial reward
        assert_eq!(policy.coinbase_reward(0), 2710000000);
        
        // After first halving
        let first_halving = policy.halving_interval;
        assert_eq!(policy.coinbase_reward(first_halving), 1355000000);
        
        // After second halving
        let second_halving = policy.halving_interval * 2;
        assert_eq!(policy.coinbase_reward(second_halving), 677500000);
    }
    
    #[test]
    fn test_total_supply_calculation() {
        let policy = MonetaryPolicy::new();
        
        // At genesis
        assert_eq!(policy.total_supply_at_height(0), 0);
        
        // After first block
        assert_eq!(policy.total_supply_at_height(1), policy.initial_reward);
        
        // After halving interval
        let expected_supply = policy.initial_reward * policy.halving_interval;
        assert_eq!(policy.total_supply_at_height(policy.halving_interval), expected_supply);
    }
    
    #[test]
    fn test_inflation_rate() {
        let policy = MonetaryPolicy::new();
        
        // Should be very high initially
        let initial_rate = policy.inflation_rate_at_height(1000);
        assert!(initial_rate > 0.0);
        
        // Should decrease over time
        let later_rate = policy.inflation_rate_at_height(100000);
        assert!(later_rate < initial_rate);
    }
    
    #[test]
    fn test_halving_calculations() {
        let policy = MonetaryPolicy::new();
        
        assert_eq!(policy.halving_epoch(0), 0);
        assert_eq!(policy.halving_epoch(policy.halving_interval), 1);
        assert_eq!(policy.halving_epoch(policy.halving_interval * 2), 2);
        
        assert_eq!(policy.blocks_until_next_halving(0), policy.halving_interval);
        assert_eq!(policy.blocks_until_next_halving(1), policy.halving_interval - 1);
    }
    
    #[test]
    fn test_fee_validation() {
        let policy = MonetaryPolicy::new();
        
        assert!(policy.is_valid_coinbase_reward(0, policy.initial_reward, 0));
        assert!(!policy.is_valid_coinbase_reward(0, policy.initial_reward + 1, 0));
        assert!(policy.is_valid_coinbase_reward(0, policy.initial_reward - 1, 0));
    }
    
    #[test]
    fn test_dust_threshold() {
        let policy = MonetaryPolicy::new();
        
        assert!(policy.is_dust(545));
        assert!(!policy.is_dust(546));
        assert!(!policy.is_dust(1000));
    }
    
    #[test]
    fn test_monetary_utils() {
        assert_eq!(MonetaryUtils::qtc_to_satoshis(1.0), 100_000_000);
        assert_eq!(MonetaryUtils::satoshis_to_qtc(100_000_000), 1.0);
        
        assert_eq!(MonetaryUtils::parse_qtc("1.0").unwrap(), 100_000_000);
        assert_eq!(MonetaryUtils::parse_qtc("0.5").unwrap(), 50_000_000);
        
        assert!(MonetaryUtils::parse_qtc("-1.0").is_err());
        assert!(MonetaryUtils::parse_qtc("invalid").is_err());
    }
    
    #[test]
    fn test_max_supply_height() {
        let policy = MonetaryPolicy::new();
        let max_height = policy.max_supply_height();
        
        // Should be a reasonable number of blocks
        assert!(max_height > 0);
        assert!(max_height < 10_000_000); // Less than 10M blocks
        
        // Supply at max height should equal or exceed max supply
        let supply_at_max = policy.total_supply_at_height(max_height);
        assert!(supply_at_max >= policy.max_supply);
    }
}
