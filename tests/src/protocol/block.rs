#[cfg(test)]
mod block_test {
    use protocol::block::Block;

    #[test]
    fn test_block_creation_and_hashing() {
        let block = Block::new([0u8; 32], 1, vec![], vec![1, 2, 3]);

        let hash1 = block.hash();
        let hash2 = block.hash();
        assert_eq!(hash1, hash2); // Hash should be deterministic
    }
}
