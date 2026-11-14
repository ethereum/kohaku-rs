//! Pairing the pool with fixed-denomination protocols.
//!
//! A fixed-denomination pool can only take multiples of its denominations, so an arbitrary amount
//! never fits exactly. The leftover has to stay public, wait, or go into a rarely used
//! denomination whose anonymity set is thin. This pool takes any amount, so it can hold the
//! leftover instead: the round part goes into the deep fixed-denomination pools, and the rest
//! comes here.
//!
//! ```
//! use kohaku_tacit::pairing::{Split, SplitOptions};
//!
//! // 1.37 ETH on Ethereum: one 1 ETH note, three 0.1 ETH notes, and 0.07 ETH left over.
//! let split = Split::for_tornado(1_370_000_000_000_000_000, 1, &SplitOptions::default());
//! assert_eq!(split.deposits.len(), 4);
//! assert_eq!(split.remainder, 70_000_000_000_000_000);
//! ```

use kohaku_tornadocash::{Asset, Pool};

/// How to split an amount between the fixed-denomination pools and this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitOptions {
    /// The most fixed-denomination deposits to plan. Each is its own transaction with its own
    /// gas, and each is one more note to keep. Default 8.
    pub max_deposits: usize,
    /// Whether to use denominations outside the canonical set. They are legitimate deployments,
    /// but they are used less, so their anonymity sets are thinner: a deposit there stands out
    /// more than the leftover would here. Default false.
    pub include_non_canonical: bool,
}

impl Default for SplitOptions {
    fn default() -> Self {
        Self {
            max_deposits: 8,
            include_non_canonical: false,
        }
    }
}

/// An amount split between fixed-denomination deposits and this pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Split {
    /// The fixed-denomination pools to deposit into, largest first, one entry per deposit.
    pub deposits: Vec<Pool>,
    /// What the denominations cannot express, for this pool. Zero when the amount is already a
    /// sum of denominations.
    pub remainder: u128,
}

impl Split {
    /// Splits `amount` wei of ETH on `chain_id` between Tornado Cash's denominations and this
    /// pool: the largest denomination that fits, repeatedly, and the rest here.
    ///
    /// Returns everything as the remainder when no denomination fits, when the chain has no ETH
    /// pools, or when `max_deposits` is zero.
    #[must_use]
    pub fn for_tornado(amount: u128, chain_id: u64, options: &SplitOptions) -> Self {
        let mut denominations: Vec<&Pool> = Pool::POOLS
            .iter()
            .filter(|pool| {
                pool.chain_id == chain_id
                    && matches!(pool.asset, Asset::Native { .. })
                    && (pool.canonical || options.include_non_canonical)
            })
            .collect();
        // Largest first, and for one denomination deployed twice prefer the canonical pool.
        denominations.sort_by_key(|pool| (std::cmp::Reverse(pool.amount_wei), !pool.canonical));
        denominations.dedup_by_key(|pool| pool.amount_wei);

        let mut deposits = Vec::new();
        let mut left = amount;
        for pool in denominations {
            while left >= pool.amount_wei && deposits.len() < options.max_deposits {
                left -= pool.amount_wei;
                deposits.push(pool.clone());
            }
        }
        Self {
            deposits,
            remainder: left,
        }
    }

    /// What the fixed-denomination deposits take, in wei.
    #[must_use]
    pub fn deposited(&self) -> u128 {
        self.deposits.iter().map(|pool| pool.amount_wei).sum()
    }

    /// Everything the split covers, in wei: the deposits plus the remainder.
    #[must_use]
    pub fn total(&self) -> u128 {
        self.deposited() + self.remainder
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ETH: u128 = 1_000_000_000_000_000_000;

    fn amounts(split: &Split) -> Vec<u128> {
        split.deposits.iter().map(|pool| pool.amount_wei).collect()
    }

    #[test]
    fn keeps_the_round_part_and_leaves_the_rest_here() {
        let split = Split::for_tornado(13 * ETH + ETH / 4, 1, &SplitOptions::default());
        assert_eq!(
            amounts(&split),
            vec![10 * ETH, ETH, ETH, ETH, ETH / 10, ETH / 10]
        );
        assert_eq!(split.remainder, ETH / 20);
        assert_eq!(split.total(), 13 * ETH + ETH / 4);
    }

    #[test]
    fn an_exact_sum_of_denominations_needs_no_remainder() {
        let split = Split::for_tornado(11 * ETH, 1, &SplitOptions::default());
        assert_eq!(amounts(&split), vec![10 * ETH, ETH]);
        assert_eq!(split.remainder, 0);
    }

    #[test]
    fn an_amount_under_the_smallest_denomination_comes_here_whole() {
        let split = Split::for_tornado(ETH / 50, 1, &SplitOptions::default());
        assert!(split.deposits.is_empty());
        assert_eq!(split.remainder, ETH / 50);

        // With the smaller non-canonical pools it fits exactly, at a thinner anonymity set.
        let options = SplitOptions {
            include_non_canonical: true,
            ..SplitOptions::default()
        };
        let split = Split::for_tornado(ETH / 50, 1, &options);
        assert_eq!(amounts(&split), vec![ETH / 100, ETH / 100]);
        assert_eq!(split.remainder, 0);
    }

    #[test]
    fn the_deposit_cap_sends_the_rest_here() {
        let options = SplitOptions {
            max_deposits: 2,
            ..SplitOptions::default()
        };
        let split = Split::for_tornado(13 * ETH, 1, &options);
        assert_eq!(amounts(&split), vec![10 * ETH, ETH]);
        assert_eq!(split.remainder, 2 * ETH);

        let options = SplitOptions {
            max_deposits: 0,
            ..SplitOptions::default()
        };
        let split = Split::for_tornado(13 * ETH, 1, &options);
        assert!(split.deposits.is_empty());
        assert_eq!(split.remainder, 13 * ETH);
    }

    #[test]
    fn only_eth_pools_of_the_named_chain_are_used() {
        let split = Split::for_tornado(150 * ETH, 1, &SplitOptions::default());
        assert!(split.deposits.iter().all(|pool| pool.chain_id == 1));
        assert!(
            split
                .deposits
                .iter()
                .all(|pool| matches!(pool.asset, Asset::Native { .. }))
        );
        assert_eq!(
            amounts(&split),
            vec![100 * ETH, 10 * ETH, 10 * ETH, 10 * ETH, 10 * ETH, 10 * ETH]
        );

        // A chain with no Tornado ETH pools leaves everything here.
        let split = Split::for_tornado(5 * ETH, 8453, &SplitOptions::default());
        assert!(split.deposits.is_empty());
        assert_eq!(split.remainder, 5 * ETH);
    }

    #[test]
    fn nothing_splits_into_nothing() {
        let split = Split::for_tornado(0, 1, &SplitOptions::default());
        assert!(split.deposits.is_empty());
        assert_eq!(split.remainder, 0);
        assert_eq!(split.total(), 0);
    }
}
