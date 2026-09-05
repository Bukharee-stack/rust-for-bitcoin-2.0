use crate::config::WalletConfig;
use crate::db::WalletDb;
use crate::error::{Result, WalletError};
use bdk_wallet::bitcoin::{Address, Amount, Network};
use bdk_wallet::{KeychainKind, Wallet};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use std::str::FromStr;

pub struct BitcoinWallet {
    pub(crate) bdk_wallet: Wallet,
    db: WalletDb,
    rpc_client: Client,
    config: WalletConfig,
}

impl BitcoinWallet {
    /// Create a new wallet with a generated mnemonic
    pub fn create_new(config: WalletConfig) -> Result<Self> {
        // Generate a new mnemonic (12 words) - requires 16 bytes of entropy
        let mut entropy = [0u8; 16];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut entropy);

        let mnemonic = bip39::Mnemonic::from_entropy_in(bip39::Language::English, &entropy)
            .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;

        println!("\n{}\n", mnemonic);

        Self::from_mnemonic(config, mnemonic.to_string())
    }

    /// Restore wallet from an existing mnemonic
    pub fn from_mnemonic(config: WalletConfig, mnemonic_str: String) -> Result<Self> {
        // Validate and parse mnemonic
        let mnemonic = bip39::Mnemonic::from_str(&mnemonic_str)
            .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;

        // Derive the extended private key (xprv) from mnemonic
        let seed = mnemonic.to_seed("");
        let xprv = bitcoin::bip32::Xpriv::new_master(config.network, &seed)
            .map_err(|e| WalletError::DescriptorError(e.to_string()))?;

        // Create descriptors for external (receiving) and internal (change) keychains
        // Using BIP84 (wpkh) standard for native segwit addresses
        let external_descriptor = format!("wpkh({}/84h/0h/0h/0/*)", xprv);
        let internal_descriptor = format!("wpkh({}/84h/0h/0h/1/*)", xprv);

        // Create BDK wallet with descriptors
        let bdk_wallet = Wallet::create(external_descriptor, internal_descriptor)
            .network(config.network)
            .create_wallet_no_persist()
            .map_err(|e| WalletError::DescriptorError(e.to_string()))?;

        // Initialize database
        let wallet_db = WalletDb::new(&config.db_path)?;

        // Store mnemonic in database (in production, this should be encrypted!)
        wallet_db.set_metadata("mnemonic", &mnemonic_str)?;
        wallet_db.set_metadata("network", &format!("{:?}", config.network))?;

        // Initialize RPC client
        let rpc_client = Client::new(
            &config.rpc_url,
            Auth::UserPass(config.rpc_user.clone(), config.rpc_password.clone()),
        )?;

        Ok(BitcoinWallet {
            bdk_wallet,
            db: wallet_db,
            rpc_client,
            config,
        })
    }

    /// Load an existing wallet from disk
    pub fn load(config: WalletConfig) -> Result<Self> {
        // Check if database exists
        if !config.db_path.exists() {
            return Err(WalletError::WalletNotFound(format!(
                "No wallet found at {:?}",
                config.db_path
            )));
        }

        let wallet_db = WalletDb::new(&config.db_path)?;

        // Retrieve mnemonic from database
        let mnemonic_str = wallet_db
            .get_metadata("mnemonic")?
            .ok_or_else(|| WalletError::WalletNotFound("Mnemonic not found in database".into()))?;

        // Recreate descriptors from mnemonic
        let mnemonic = bip39::Mnemonic::from_str(&mnemonic_str)
            .map_err(|e| WalletError::InvalidMnemonic(e.to_string()))?;
        let seed = mnemonic.to_seed("");
        let xprv = bitcoin::bip32::Xpriv::new_master(config.network, &seed)
            .map_err(|e| WalletError::DescriptorError(e.to_string()))?;

        let external_descriptor = format!("wpkh({}/84h/0h/0h/0/*)", xprv);
        let internal_descriptor = format!("wpkh({}/84h/0h/0h/1/*)", xprv);

        // Create wallet from descriptors
        let bdk_wallet = Wallet::create(external_descriptor, internal_descriptor)
            .network(config.network)
            .create_wallet_no_persist()
            .map_err(|e| WalletError::DescriptorError(e.to_string()))?;

        // Initialize RPC client
        let rpc_client = Client::new(
            &config.rpc_url,
            Auth::UserPass(config.rpc_user.clone(), config.rpc_password.clone()),
        )?;

        Ok(BitcoinWallet {
            bdk_wallet,
            db: wallet_db,
            rpc_client,
            config,
        })
    }

    /// Generate a new receiving address
    pub fn get_new_address(&mut self) -> Result<Address> {
        let address_info = self.bdk_wallet.reveal_next_address(KeychainKind::External);
        let address = address_info.address.clone();

        // Store in database
        self.db
            .store_address(&address.to_string(), "external", address_info.index, false)?;

        Ok(address)
    }

    /// Generate a new change address (internal)
    pub fn get_change_address(&mut self) -> Result<Address> {
        let address_info = self.bdk_wallet.reveal_next_address(KeychainKind::Internal);
        let address = address_info.address.clone();

        // Store in database
        self.db
            .store_address(&address.to_string(), "internal", address_info.index, false)?;

        Ok(address)
    }

    /// Get the current balance (confirmed + unconfirmed)
    pub fn get_balance(&self) -> Result<(Amount, Amount)> {
        let balance = self.bdk_wallet.balance();
        Ok((balance.confirmed, balance.total()))
    }

    /// Sync wallet with the blockchain via RPC
    pub fn sync(&mut self) -> Result<()> {
        // Verify RPC connection
        let blockchain_info = self.rpc_client.get_blockchain_info()?;
        
        println!("🔄 Syncing with {} at height {}...", blockchain_info.chain, blockchain_info.blocks);

        // Get all our addresses
        let external_addresses: Vec<_> = (0..20)
            .map(|i| self.bdk_wallet.peek_address(KeychainKind::External, i))
            .collect();

        let internal_addresses: Vec<_> = (0..20)
            .map(|i| self.bdk_wallet.peek_address(KeychainKind::Internal, i))
            .collect();

        // Store addresses in database
        for (keychain_name, addresses) in [
            ("external", &external_addresses),
            ("internal", &internal_addresses),
        ] {
            for (idx, address_info) in addresses.iter().enumerate() {
                let address = &address_info.address;

                // Store the address in our database
                self.db
                    .store_address(&address.to_string(), keychain_name, idx as u32, false)?;
            }
        }

        // Use listunspent to get UTXOs (this requires addresses to be imported/watched)
        // For regtest, Bitcoin Core tracks all addresses by default
        match self.rpc_client.list_unspent(None, None, None, None, None) {
            Ok(unspent) => {
                let our_addresses: std::collections::HashSet<_> = external_addresses
                    .iter()
                    .chain(internal_addresses.iter())
                    .map(|a| a.address.to_string())
                    .collect();

                for utxo in unspent {
                    if let Some(addr) = &utxo.address {
                        let addr_str = addr.clone().assume_checked().to_string();
                        if our_addresses.contains(&addr_str) {
                            // Calculate block height from confirmations
                            let height = if utxo.confirmations > 0 {
                                Some(blockchain_info.blocks.saturating_sub(utxo.confirmations as u64 - 1) as u32)
                            } else {
                                None
                            };
                            
                            self.db.store_utxo(
                                &utxo.txid.to_string(),
                                utxo.vout,
                                utxo.amount.to_sat(),
                                &addr_str,
                                height,
                            )?;
                        }
                    }
                }
            
            }
            Err(e) => {
                println!("could not list unspent outputs: {}", e);
            
            }
        }

        Ok(())
    }

    /// List all addresses (external and internal)
    pub fn list_addresses(&self, keychain: Option<&str>) -> Result<Vec<crate::db::AddressRecord>> {
        self.db.get_addresses(keychain)
    }

    /// List all unspent UTXOs
    pub fn list_utxos(&self) -> Result<Vec<crate::db::UtxoRecord>> {
        self.db.get_unspent_utxos()
    }

    /// Get network
    pub fn network(&self) -> Network {
        self.config.network
    }

    /// Get RPC client reference
    pub fn rpc_client(&self) -> &Client {
        &self.rpc_client
    }

    /// Get database reference
    pub fn db(&self) -> &WalletDb {
        &self.db
    }
}
