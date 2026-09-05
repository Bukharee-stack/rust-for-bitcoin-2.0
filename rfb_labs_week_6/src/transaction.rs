use crate::error::{Result, WalletError};
use crate::wallet::BitcoinWallet;
use bdk_wallet::bitcoin::{Address, Amount, FeeRate};
use bitcoincore_rpc::RpcApi;
use log::{info, warn};
use std::str::FromStr;

impl BitcoinWallet {
    /// Create, sign, and broadcast a transaction
    pub fn send_to_address(
        &mut self,
        recipient: &str,
        amount_sat: u64,
        fee_rate_sat_vb: Option<f64>,
    ) -> Result<String> {
        info!("Preparing to send {} sats to {}", amount_sat, recipient);

        // Parse recipient address
        let recipient_address = Address::from_str(recipient)
            .map_err(WalletError::AddressError)?
            .require_network(self.network())
            .map_err(|e| {
                WalletError::TransactionError(format!("Address network mismatch: {}", e))
            })?;

        // Check balance
        let balance = self.bdk_wallet.balance();
        if balance.total().to_sat() < amount_sat {
            return Err(WalletError::InsufficientFunds {
                available: balance.total().to_sat(),
                required: amount_sat,
            });
        }

        info!("Current balance: {} sats", balance.total().to_sat());

        // Determine fee rate first
        let fee_rate = if let Some(rate) = fee_rate_sat_vb {
            let fr = FeeRate::from_sat_per_vb(rate.ceil() as u64)
                .ok_or_else(|| WalletError::TransactionError("Invalid fee rate".to_string()))?;
            info!("Using fee rate: {} sat/vB", rate);
            fr
        } else {
            // Try to estimate fee from RPC
            match self.estimate_smart_fee(6) {
                Ok(estimated_rate) => {
                    info!(
                        "Using estimated fee rate: {} sat/vB",
                        estimated_rate.to_sat_per_vb_ceil()
                    );
                    estimated_rate
                }
                Err(e) => {
                    warn!("Could not estimate fee: {}. Using default 1 sat/vB", e);
                    FeeRate::from_sat_per_vb(1).ok_or_else(|| {
                        WalletError::TransactionError("Invalid fee rate".to_string())
                    })?
                }
            }
        };

        // Build the transaction using BDK
        let mut tx_builder = self.bdk_wallet.build_tx();

        // Add recipient
        tx_builder.add_recipient(
            recipient_address.script_pubkey(),
            Amount::from_sat(amount_sat),
        );

        // Set fee rate
        tx_builder.fee_rate(fee_rate);

        // Enable RBF (Replace-By-Fee) - note: RBF is enabled by default in BDK
        // tx_builder.enable_rbf(); // Not needed, enabled by default

        // Build the transaction
        let mut psbt = tx_builder.finish().map_err(|e| {
            WalletError::TransactionError(format!("Failed to build transaction: {}", e))
        })?;

        info!("Transaction built successfully");

        // Sign the transaction
        let finalized = self
            .bdk_wallet
            .sign(&mut psbt, bdk_wallet::SignOptions::default())
            .map_err(|e| {
                WalletError::TransactionError(format!("Failed to sign transaction: {}", e))
            })?;

        if !finalized {
            return Err(WalletError::TransactionError(
                "Transaction could not be finalized".to_string(),
            ));
        }

        info!("Transaction signed successfully");

        // Calculate fee before extracting
        let fee = psbt.fee().map_err(|e| {
            WalletError::TransactionError(format!("Failed to calculate fee: {}", e))
        })?;

        // Extract the final transaction
        let tx = psbt.extract_tx().map_err(|e| {
            WalletError::TransactionError(format!("Failed to extract transaction: {}", e))
        })?;

        let txid = tx.compute_txid();

        info!("Transaction ID: {}", txid);
        info!("Fee: {} sats", fee.to_sat());

        // Broadcast the transaction
        self.broadcast_transaction(&tx)?;

        // Store transaction in database
        let raw_tx = bitcoin::consensus::encode::serialize_hex(&tx);
        self.db().store_transaction(
            &txid.to_string(),
            &raw_tx,
            None,                                       // Not yet confirmed
            -(amount_sat as i64 + fee.to_sat() as i64), // Negative for outgoing
            Some(fee.to_sat()),
            false,
        )?;

        info!("Transaction broadcast successfully!");

        Ok(txid.to_string())
    }

    /// Broadcast a transaction to the network
    fn broadcast_transaction(&self, tx: &bdk_wallet::bitcoin::Transaction) -> Result<()> {
        info!("Broadcasting transaction...");

        self.rpc_client()
            .send_raw_transaction(tx)
            .map_err(|e| WalletError::TransactionError(format!("Failed to broadcast: {}", e)))?;

        Ok(())
    }

    /// Estimate smart fee from Bitcoin Core
    fn estimate_smart_fee(&self, conf_target: u16) -> Result<FeeRate> {
        let estimate = self
            .rpc_client()
            .estimate_smart_fee(conf_target, None)
            .map_err(WalletError::RpcError)?;

        if let Some(fee_rate) = estimate.fee_rate {
            // Convert from BTC/kB to sat/vB
            let sat_per_kvb = fee_rate.to_sat();
            let sat_per_vb = (sat_per_kvb as f64 / 1000.0).ceil() as u64;
            FeeRate::from_sat_per_vb(sat_per_vb).ok_or_else(|| {
                WalletError::TransactionError("Invalid fee rate from estimate".to_string())
            })
        } else {
            Err(WalletError::TransactionError(
                "No fee estimate available".to_string(),
            ))
        }
    }

    /// Create a transaction that sends all funds (sweep)
    pub fn sweep_to_address(
        &mut self,
        recipient: &str,
        fee_rate_sat_vb: Option<f64>,
    ) -> Result<String> {
        info!("Preparing to sweep all funds to {}", recipient);

        // Parse recipient address
        let recipient_address = Address::from_str(recipient)
            .map_err(WalletError::AddressError)?
            .require_network(self.network())
            .map_err(|e| {
                WalletError::TransactionError(format!("Address network mismatch: {}", e))
            })?;

        // Estimate fee first if needed
        let fee_rate = if let Some(fee_rate) = fee_rate_sat_vb {
            FeeRate::from_sat_per_vb(fee_rate.ceil() as u64)
                .ok_or_else(|| WalletError::TransactionError("Invalid fee rate".to_string()))?
        } else {
            self.estimate_smart_fee(6)
                .unwrap_or_else(|_| FeeRate::from_sat_per_vb(1).expect("Valid fee rate"))
        };

        // Build the transaction using BDK
        let mut tx_builder = self.bdk_wallet.build_tx();

        // Drain wallet (send all)
        tx_builder
            .drain_wallet()
            .drain_to(recipient_address.script_pubkey());

        // Set fee rate
        tx_builder.fee_rate(fee_rate);

        // Build the transaction
        let mut psbt = tx_builder.finish().map_err(|e| {
            WalletError::TransactionError(format!("Failed to build sweep transaction: {}", e))
        })?;

        // Sign the transaction
        let finalized = self
            .bdk_wallet
            .sign(&mut psbt, bdk_wallet::SignOptions::default())
            .map_err(|e| {
                WalletError::TransactionError(format!("Failed to sign transaction: {}", e))
            })?;

        if !finalized {
            return Err(WalletError::TransactionError(
                "Transaction could not be finalized".to_string(),
            ));
        }

        // Extract and broadcast
        let fee = psbt.fee().map_err(|e| {
            WalletError::TransactionError(format!("Failed to calculate fee: {}", e))
        })?;

        let tx = psbt.extract_tx().map_err(|e| {
            WalletError::TransactionError(format!("Failed to extract transaction: {}", e))
        })?;
        let txid = tx.compute_txid();

        self.broadcast_transaction(&tx)?;

        // Store in database
        let raw_tx = bitcoin::consensus::encode::serialize_hex(&tx);

        self.db().store_transaction(
            &txid.to_string(),
            &raw_tx,
            None,
            -(fee.to_sat() as i64),
            Some(fee.to_sat()),
            false,
        )?;

        info!("Sweep transaction broadcast successfully!");

        Ok(txid.to_string())
    }

    /// Get transaction history
    pub fn get_transaction_history(
        &self,
        limit: Option<usize>,
    ) -> Result<Vec<crate::db::TransactionRecord>> {
        self.db().get_transactions(limit)
    }

    /// Check if a transaction is confirmed
    pub fn is_transaction_confirmed(&self, txid: &str) -> Result<bool> {
        match self.rpc_client().get_raw_transaction_info(
            &bitcoin::Txid::from_str(txid)
                .map_err(|e| WalletError::TransactionError(e.to_string()))?,
            None,
        ) {
            Ok(tx_info) => Ok(tx_info.confirmations.unwrap_or(0) > 0),
            Err(_) => Ok(false),
        }
    }
}
