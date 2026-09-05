use crate::config::WalletConfig;
use crate::error::Result;
use crate::wallet::BitcoinWallet;
use bitcoincore_rpc::RpcApi;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bitcoin-wallet")]
#[command(about = "A Bitcoin wallet built with Rust, BDK, and bitcoincore-rpc", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a new wallet with a generated mnemonic
    Create,

    /// Restore a wallet from an existing mnemonic
    Restore {
        /// The 12 or 24 word mnemonic phrase (space-separated)
        #[arg(required = true)]
        mnemonic: Vec<String>,
    },

    /// Get wallet balance
    Balance,

    /// Generate a new receiving address
    NewAddress,

    /// List all addresses (external and internal)
    ListAddresses {
        /// Filter by keychain: external or internal
        #[arg(short, long)]
        keychain: Option<String>,
    },

    /// List all unspent UTXOs
    ListUtxos,

    /// Sync wallet with the blockchain
    Sync,

    /// Send Bitcoin to an address
    Send {
        /// Recipient address
        #[arg(short, long)]
        to: String,

        /// Amount in satoshis
        #[arg(short, long)]
        amount: u64,

        /// Fee rate in sat/vB (optional, will estimate if not provided)
        #[arg(short, long)]
        fee_rate: Option<f64>,
    },


    /// Show transaction history
    History {
        /// Maximum number of transactions to show
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },

    /// Check if a transaction is confirmed
    CheckTx {
        /// Transaction ID
        txid: String,
    },

    /// Show wallet information
    Info,
}

impl Cli {
    pub fn run() -> Result<()> {
        env_logger::init();
        let cli = Cli::parse();

        match cli.command {
            Commands::Create => {
                let config = WalletConfig::from_env()?;
                config.validate()?;

                let _wallet = BitcoinWallet::create_new(config)?;
            }

            Commands::Restore { mnemonic } => {
                let config = WalletConfig::from_env()?;
                config.validate()?;

                let mnemonic_str = mnemonic.join(" ");

                let _wallet = BitcoinWallet::from_mnemonic(config, mnemonic_str)?;
            }

            Commands::Balance => {
                let config = WalletConfig::from_env()?;
                let wallet = BitcoinWallet::load(config)?;

                let (confirmed, total) = wallet.get_balance()?;

                println!(
                    "   confirmed: {} sats ({} BTC)",
                    confirmed.to_sat(),
                    confirmed.to_btc()
                );
                println!(
                    "   total:     {} sats ({} BTC)",
                    total.to_sat(),
                    total.to_btc()
                );

                if total.to_sat() > confirmed.to_sat() {
                    println!("   Pending:   {} sats", total.to_sat() - confirmed.to_sat());
                }
            }

            Commands::NewAddress => {
                let config = WalletConfig::from_env()?;
                let mut wallet = BitcoinWallet::load(config)?;

                let address = wallet.get_new_address()?;
                println!("   {}", address);
            }

            Commands::ListAddresses { keychain } => {
                let config = WalletConfig::from_env()?;
                let wallet = BitcoinWallet::load(config)?;

                let addresses = wallet.list_addresses(keychain.as_deref())?;

                if addresses.is_empty() {
                    return Ok(());
                }

                for addr in addresses {
                    let status = if addr.used { "used" } else { "unused" };
                    println!(
                        "   [{}] #{}: {} ({})",
                        addr.keychain, addr.index, addr.address, status
                    );
                }
            }

            Commands::ListUtxos => {
                let config = WalletConfig::from_env()?;
                let wallet = BitcoinWallet::load(config)?;

                let utxos = wallet.list_utxos()?;

                if utxos.is_empty() {
                    println!("No UTXOs found.");
                    return Ok(());
                }

                for utxo in utxos {
                    let height_str = utxo
                        .block_height
                        .map(|h| h.to_string())
                        .unwrap_or_else(|| "unconfirmed".to_string());

                    println!(
                        "   {}:{} - {} sats (block: {})",
                        utxo.txid, utxo.vout, utxo.amount, height_str
                    );
                }
            }

            Commands::Sync => {
                let config = WalletConfig::from_env()?;
                let mut wallet = BitcoinWallet::load(config)?;

                wallet.sync()?;

                let (_confirmed, total) = wallet.get_balance()?;

                println!("   Balance: {} sats", total.to_sat());
            }

            Commands::Send {
                to,
                amount,
                fee_rate,
            } => {
                let config = WalletConfig::from_env()?;
                let mut wallet = BitcoinWallet::load(config)?;

                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .expect("Failed to read input");

                if input.trim().to_lowercase() != "yes" {
                    return Ok(());
                }

                let txid = wallet.send_to_address(&to, amount, fee_rate)?;

                println!("   TXID: {}", txid);
            }


            Commands::History { limit } => {
                let config = WalletConfig::from_env()?;
                let wallet = BitcoinWallet::load(config)?;

                let txs = wallet.get_transaction_history(Some(limit))?;

                if txs.is_empty() {
                    println!("No transactions found.");
                    return Ok(());
                }
            }

            Commands::CheckTx { txid } => {
                let config = WalletConfig::from_env()?;
                let wallet = BitcoinWallet::load(config)?;

                let confirmed = wallet.is_transaction_confirmed(&txid)?;

                if confirmed {
                    println!("transaction {} confirmed", txid);
                } else {
                    println!("transaction {} not yet confirmed", txid);
                }
            }

            Commands::Info => {
                let config = WalletConfig::from_env()?;
                let wallet = BitcoinWallet::load(config.clone())?;

                println!("   network:     {:?}", wallet.network());
                println!("   database:    {:?}", config.db_path);
                println!("   wallet path: {:?}", config.wallet_path);

                // Try to get blockchain info from RPC
                match wallet.rpc_client().get_blockchain_info() {
                    Ok(info) => {
                        println!("   chain:       {}", info.chain);
                        println!("   blocks:      {}", info.blocks);
                        println!("   difficulty:  {}", info.difficulty);
                    }
                    Err(e) => {
                        println!("Could not connect to Bitcoin node: {}", e);
                    }
                }

                let (confirmed, total) = wallet.get_balance()?;
                println!("   confirmed:   {} sats", confirmed.to_sat());
                println!("   total:       {} sats", total.to_sat());
            }
        }

        Ok(())
    }
}
