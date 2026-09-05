use crate::error::Result;
use rusqlite::{params, Connection};
use std::path::Path;

pub struct WalletDb {
    conn: Connection,
}

impl WalletDb {
    /// Create or open a wallet database at the specified path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = WalletDb { conn };
        db.initialize_schema()?;
        Ok(db)
    }

    /// Initialize database schema for wallet metadata
    fn initialize_schema(&self) -> Result<()> {
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS wallet_metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS addresses (
                address TEXT PRIMARY KEY,
                keychain TEXT NOT NULL,
                idx INTEGER NOT NULL,
                used INTEGER DEFAULT 0,
                created_at INTEGER NOT NULL
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS utxos (
                txid TEXT NOT NULL,
                vout INTEGER NOT NULL,
                amount INTEGER NOT NULL,
                address TEXT NOT NULL,
                spent INTEGER DEFAULT 0,
                block_height INTEGER,
                PRIMARY KEY (txid, vout)
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS transactions (
                txid TEXT PRIMARY KEY,
                raw_tx TEXT NOT NULL,
                block_height INTEGER,
                timestamp INTEGER NOT NULL,
                amount INTEGER NOT NULL,
                fee INTEGER,
                confirmed INTEGER DEFAULT 0
            )",
            [],
        )?;

        Ok(())
    }

    /// Store or update wallet metadata
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO wallet_metadata (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Retrieve wallet metadata
    pub fn get_metadata(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT value FROM wallet_metadata WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;

        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Store a generated address
    pub fn store_address(
        &self,
        address: &str,
        keychain: &str,
        index: u32,
        used: bool,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO addresses (address, keychain, idx, used, created_at) 
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![address, keychain, index, used as i32, now],
        )?;
        Ok(())
    }

    /// Mark an address as used
    pub fn mark_address_used(&self, address: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE addresses SET used = 1 WHERE address = ?1",
            params![address],
        )?;
        Ok(())
    }

    /// Get all addresses for a keychain
    pub fn get_addresses(&self, keychain: Option<&str>) -> Result<Vec<AddressRecord>> {
        let query = if let Some(kc) = keychain {
            format!(
                "SELECT address, keychain, idx, used FROM addresses WHERE keychain = '{}' ORDER BY idx",
                kc
            )
        } else {
            "SELECT address, keychain, idx, used FROM addresses ORDER BY keychain, idx"
                .to_string()
        };

        let mut stmt = self.conn.prepare(&query)?;
        let rows = stmt.query_map([], |row| {
            Ok(AddressRecord {
                address: row.get(0)?,
                keychain: row.get(1)?,
                index: row.get(2)?,
                used: row.get::<_, i32>(3)? != 0,
            })
        })?;

        let mut addresses = Vec::new();
        for addr in rows {
            addresses.push(addr?);
        }
        Ok(addresses)
    }

    /// Store a UTXO
    pub fn store_utxo(
        &self,
        txid: &str,
        vout: u32,
        amount: u64,
        address: &str,
        block_height: Option<u32>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO utxos (txid, vout, amount, address, spent, block_height) 
             VALUES (?1, ?2, ?3, ?4, 0, ?5)",
            params![
                txid,
                vout,
                amount as i64,
                address,
                block_height.map(|h| h as i64)
            ],
        )?;
        Ok(())
    }

    /// Mark a UTXO as spent
    pub fn mark_utxo_spent(&self, txid: &str, vout: u32) -> Result<()> {
        self.conn.execute(
            "UPDATE utxos SET spent = 1 WHERE txid = ?1 AND vout = ?2",
            params![txid, vout],
        )?;
        Ok(())
    }

    /// Get all unspent UTXOs
    pub fn get_unspent_utxos(&self) -> Result<Vec<UtxoRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT txid, vout, amount, address, block_height FROM utxos WHERE spent = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(UtxoRecord {
                txid: row.get(0)?,
                vout: row.get(1)?,
                amount: row.get::<_, i64>(2)? as u64,
                address: row.get(3)?,
                block_height: row.get::<_, Option<i64>>(4)?.map(|h| h as u32),
            })
        })?;

        let mut utxos = Vec::new();
        for utxo in rows {
            utxos.push(utxo?);
        }
        Ok(utxos)
    }

    /// Calculate total balance from unspent UTXOs
    pub fn get_balance(&self) -> Result<u64> {
        let mut stmt = self
            .conn
            .prepare("SELECT COALESCE(SUM(amount), 0) FROM utxos WHERE spent = 0")?;
        let balance: i64 = stmt.query_row([], |row| row.get(0))?;
        Ok(balance as u64)
    }

    /// Store a transaction
    pub fn store_transaction(
        &self,
        txid: &str,
        raw_tx: &str,
        block_height: Option<u32>,
        amount: i64,
        fee: Option<u64>,
        confirmed: bool,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO transactions (txid, raw_tx, block_height, timestamp, amount, fee, confirmed) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                txid,
                raw_tx,
                block_height.map(|h| h as i64),
                now,
                amount,
                fee.map(|f| f as i64),
                confirmed as i32
            ],
        )?;
        Ok(())
    }

    /// Get transaction history
    pub fn get_transactions(&self, limit: Option<usize>) -> Result<Vec<TransactionRecord>> {
        let query = if let Some(lim) = limit {
            format!(
                "SELECT txid, raw_tx, block_height, timestamp, amount, fee, confirmed 
                 FROM transactions ORDER BY timestamp DESC LIMIT {}",
                lim
            )
        } else {
            "SELECT txid, raw_tx, block_height, timestamp, amount, fee, confirmed 
             FROM transactions ORDER BY timestamp DESC"
                .to_string()
        };

        let mut stmt = self.conn.prepare(&query)?;
        let rows = stmt.query_map([], |row| {
            Ok(TransactionRecord {
                txid: row.get(0)?,
                raw_tx: row.get(1)?,
                block_height: row.get::<_, Option<i64>>(2)?.map(|h| h as u32),
                timestamp: row.get(3)?,
                amount: row.get(4)?,
                fee: row.get::<_, Option<i64>>(5)?.map(|f| f as u64),
                confirmed: row.get::<_, i32>(6)? != 0,
            })
        })?;

        let mut transactions = Vec::new();
        for tx in rows {
            transactions.push(tx?);
        }
        Ok(transactions)
    }
}

#[derive(Debug, Clone)]
pub struct AddressRecord {
    pub address: String,
    pub keychain: String,
    pub index: u32,
    pub used: bool,
}

#[derive(Debug, Clone)]
pub struct UtxoRecord {
    pub txid: String,
    pub vout: u32,
    pub amount: u64,
    pub address: String,
    pub block_height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct TransactionRecord {
    pub txid: String,
    pub raw_tx: String,
    pub block_height: Option<u32>,
    pub timestamp: i64,
    pub amount: i64,
    pub fee: Option<u64>,
    pub confirmed: bool,
}
