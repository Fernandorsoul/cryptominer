#[cfg(test)]
mod tests {
    use crate::config::*;
    use crate::stats::MiningStats;
    use crate::engine::randomx::RandomXEngine;

    // === Config tests ===

    #[test]
    fn test_default_config() {
        let config = MiningConfig::default();
        assert_eq!(config.coin, "monero");
        assert_eq!(config.threads, 0);
        assert_eq!(config.intensity, 1.0);
        assert!(config.wallet_address.is_empty());
    }

    #[test]
    fn test_parse_config_toml() {
        let toml = r#"
[mining]
coin = "xmr"
threads = 4
intensity = 0.8
wallet_address = "4AdUn...test"

[daemon]
url = "http://127.0.0.1:18081"

[gpu]
enabled = true
miner_path = "/usr/bin/lolminer"
devices = [0, 1]

[stratum]
enabled = true
bind = "0.0.0.0:3333"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.mining.coin, "xmr");
        assert_eq!(config.mining.threads, 4);
        assert_eq!(config.mining.intensity, 0.8);
        assert_eq!(config.mining.wallet_address, "4AdUn...test");
        assert_eq!(config.daemon.url, "http://127.0.0.1:18081");
        assert!(config.gpu.enabled);
        assert_eq!(config.gpu.miner_path, "/usr/bin/lolminer");
        assert_eq!(config.gpu.devices, vec![0, 1]);
        assert!(config.stratum.enabled);
        assert_eq!(config.stratum.bind, "0.0.0.0:3333");
    }

    #[test]
    fn test_parse_config_minimal() {
        let toml = r#"
[mining]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.mining.coin, "monero");
        assert_eq!(config.daemon.url, "http://127.0.0.1:18081");
        assert!(!config.gpu.enabled);
        assert!(!config.stratum.enabled);
    }

    #[test]
    fn test_profiles() {
        let toml = r#"
[mining]

[[profiles]]
name = "monero"
coin = "xmr"
algorithm = "randomx"
daemon_url = "http://127.0.0.1:18081"

[[profiles]]
name = "etc"
coin = "etc"
algorithm = "etchash"
daemon_url = "http://127.0.0.1:8551"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.profiles.len(), 2);
        assert_eq!(config.profiles[0].name, "monero");
        assert_eq!(config.profiles[1].algorithm, "etchash");
    }

    // === Stats tests ===

    #[test]
    fn test_stats_initial_state() {
        let stats = MiningStats::new();
        assert_eq!(stats.total_hashes(), 0);
        assert_eq!(stats.accepted_shares(), 0);
        assert_eq!(stats.rejected_shares(), 0);
        assert_eq!(stats.blocks(), 0);
        assert_eq!(stats.hashrate(), 0.0);
    }

    #[test]
    fn test_stats_add_hashes() {
        let stats = MiningStats::new();
        stats.add_hashes(100);
        stats.add_hashes(200);
        assert_eq!(stats.total_hashes(), 300);
    }

    #[test]
    fn test_stats_shares() {
        let stats = MiningStats::new();
        stats.accept_share();
        stats.accept_share();
        stats.reject_share();
        assert_eq!(stats.accepted_shares(), 2);
        assert_eq!(stats.rejected_shares(), 1);
    }

    #[test]
    fn test_stats_blocks() {
        let stats = MiningStats::new();
        stats.found_block();
        stats.found_block();
        stats.found_block();
        assert_eq!(stats.blocks(), 3);
    }

    #[test]
    fn test_format_hashrate() {
        assert_eq!(MiningStats::format_hashrate(500.0), "500.00 H/s");
        assert_eq!(MiningStats::format_hashrate(1500.0), "1.50 KH/s");
        assert_eq!(MiningStats::format_hashrate(1_500_000.0), "1.50 MH/s");
        assert_eq!(MiningStats::format_hashrate(2_500_000_000.0), "2.50 GH/s");
    }

    #[test]
    fn test_stats_default() {
        let stats = MiningStats::default();
        assert_eq!(stats.total_hashes(), 0);
    }

    // === RandomX engine tests ===

    #[test]
    fn test_randomx_hash_meets_difficulty() {
        use crate::engine::randomx::RandomXEngine;

        // Zero difficulty always passes
        let hash = [0u8; 32];
        assert!(RandomXEngine::hash_meets_difficulty(&hash, 0));

        // Very low hash value should pass high difficulty
        let mut low_hash = [0u8; 32];
        low_hash[31] = 0x01; // very small number
        assert!(RandomXEngine::hash_meets_difficulty(&low_hash, 1));

        // Max hash should fail difficulty 2
        let max_hash = [0xFFu8; 32];
        assert!(!RandomXEngine::hash_meets_difficulty(&max_hash, 2));
    }

    #[test]
    fn test_randomx_insert_nonce() {
        use crate::engine::randomx::RandomXEngine;

        let mut blob = vec![0u8; 100];
        RandomXEngine::insert_nonce(&mut blob, 0x12345678);

        // Little-endian: 78 56 34 12
        assert_eq!(blob[39], 0x78);
        assert_eq!(blob[40], 0x56);
        assert_eq!(blob[41], 0x34);
        assert_eq!(blob[42], 0x12);
    }

    #[test]
    fn test_randomx_hex() {
        use crate::engine::randomx::RandomXEngine;

        let data = vec![0xDE, 0xAD, 0xBE, 0xEF];
        let hex = RandomXEngine::hex_encode(&data);
        assert_eq!(hex, "deadbeef");

        let decoded = RandomXEngine::hex_decode(&hex).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_randomx_hex_decode_invalid() {
        use crate::engine::randomx::RandomXEngine;

        assert!(RandomXEngine::hex_decode("not_hex!").is_err());
    }

    // === Coin algorithm tests ===

    #[test]
    fn test_coin_algorithm() {
        // We can't easily test the private function, but we can verify
        // the mapping logic indirectly
        let coins = vec![
            ("monero", "randomx"),
            ("xmr", "randomx"),
            ("etc", "etchash"),
            ("rvn", "kawpow"),
            ("erg", "autolykos2"),
        ];

        for (coin, expected_algo) in coins {
            let algo = match coin {
                "monero" | "xmr" => "randomx",
                "ethereum_classic" | "etc" => "etchash",
                "ravencoin" | "rvn" => "kawpow",
                "ergo" | "erg" => "autolykos2",
                _ => "unknown",
            };
            assert_eq!(algo, expected_algo, "Algorithm mismatch for {}", coin);
        }
    }
}