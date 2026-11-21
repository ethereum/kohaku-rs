use kohaku_railgun::TOTAL_LEAVES;

/// Normalize a tree number and leaf index to ensure the leaf index isn't overflowing the tree
pub fn normalize_tree_position(tree_number: u32, leaf_index: u32) -> (u32, u32) {
    let global = u64::from(tree_number) * u64::from(TOTAL_LEAVES) + u64::from(leaf_index);

    let normalized_tree_number = (global / u64::from(TOTAL_LEAVES)) as u32;
    let normalized_leaf_index = (global % u64::from(TOTAL_LEAVES)) as u32;

    (normalized_tree_number, normalized_leaf_index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_position() {
        // No overflow
        assert_eq!(normalize_tree_position(0, 0), (0, 0));
        assert_eq!(normalize_tree_position(1, 10), (1, 10));

        // Overflow
        assert_eq!(normalize_tree_position(0, TOTAL_LEAVES), (1, 0));
        assert_eq!(normalize_tree_position(2, TOTAL_LEAVES + 5), (3, 5));
    }
}
