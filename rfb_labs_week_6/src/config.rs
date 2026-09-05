use crate::error::{Result, WalletError};
use bitcoin::Network;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct WalletConfig {
    pub network: Network,
    pub rpc_url: String,
    pub rpc_user: String,
    pub rpc_password: String,
    pub wallet_path: PathBuf,
    pub db_path: PathBuf,
}

impl WalletConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        // Try to load .env from current directory or parent directories
        if let Err(e) = dotenv::dotenv() {
            eprintln!("Warning: Could not load .env file: {}", e);
        }

        let network_str = env::var("BITCOIN_NETWORK").unwrap_or_else(|_| "regtest".to_string());
        let network = match network_str.to_lowercase().as_str() {
            "bitcoin" | "mainnet" => Network::Bitcoin,
            "testnet" => Network::Testnet,
            "signet" => Network::Signet,
            "regtest" => Network::Regtest,
            _ => {
                return Err(WalletError::InvalidNetwork(format!(
                    "Unknown network: {}",
                    network_str
                )))
            }
        };

        let rpc_url =
            env::var("BITCOIN_RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:18443".to_string());
        let rpc_user = env::var("BITCOIN_RPC_USER").unwrap_or_else(|_| "bitcoinrpc".to_string());
        let rpc_password =
            env::var("BITCOIN_RPC_PASSWORD").unwrap_or_else(|_| "password".to_string());

        let wallet_dir = env::var("WALLET_DIR").unwrap_or_else(|_| "./wallet_data".to_string());
        let wallet_path = PathBuf::from(&wallet_dir);

        // Create wallet directory if it doesn't exist
        if !wallet_path.exists() {
            std::fs::create_dir_all(&wallet_path).map_err(|e| {
                WalletError::ConfigError(format!("Failed to create wallet directory: {}", e))
            })?;
        }

        let db_path = wallet_path.join("wallet.db");

        Ok(WalletConfig {
            network,
            rpc_url,
            rpc_user,
            rpc_password,
            wallet_path,
            db_path,
        })
    }

    /// Create a default configuration for regtest
    pub fn default_regtest() -> Self {
        let wallet_path = PathBuf::from("./wallet_data");
        std::fs::create_dir_all(&wallet_path).ok();

        WalletConfig {
            network: Network::Regtest,
            rpc_url: "http://127.0.0.1:18443".to_string(),
            rpc_user: "bitcoinrpc".to_string(),
            rpc_password: "password".to_string(),
            wallet_path: wallet_path.clone(),
            db_path: wallet_path.join("wallet.db"),
        }
    }

    /// Get the BDK file store path
    pub fn get_bdk_store_path(&self) -> PathBuf {
        self.wallet_path.join("bdk_wallet.db")
    }

    /// Validate that the configuration is usable
    pub fn validate(&self) -> Result<()> {
        // Check that wallet directory exists and is writable
        if !self.wallet_path.exists() {
            return Err(WalletError::ConfigError(format!(
                "Wallet directory does not exist: {:?}",
                self.wallet_path
            )));
        }

        // Check RPC URL format
        if !self.rpc_url.starts_with("http://") && !self.rpc_url.starts_with("https://") {
            return Err(WalletError::ConfigError(
                "RPC URL must start with http:// or https://".to_string(),
            ));
        }

        Ok(())
    }

    /// Display configuration summary (without sensitive data)
    pub fn summary(&self) -> String {
        format!(
            "Network: {:?}\nRPC URL: {}\nWallet Path: {:?}\nDatabase: {:?}",
            self.network, self.rpc_url, self.wallet_path, self.db_path
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_regtest_config() {
        let config = WalletConfig::default_regtest();
        assert_eq!(config.network, Network::Regtest);
        assert!(config.validate().is_ok());
    }
}
