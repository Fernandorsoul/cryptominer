use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{debug, info, warn};

/// Mining event stored in the database.
#[derive(Debug, Clone, Serialize)]
pub struct ShareRecord {
    pub id: i64,
    pub timestamp: i64,
    pub event_type: String, // "accepted", "rejected", "block"
    pub job_id: String,
    pub nonce: String,
    pub hash: String,
    pub difficulty: u64,
    pub height: u64,
}

/// Periodic hashrate snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct HashrateSnapshot {
    pub id: i64,
    pub timestamp: i64,
    pub hashrate: f64,
    pub threads: u32,
    pub accepted: u64,
    pub rejected: u64,
}

/// Block found record.
#[derive(Debug, Clone, Serialize)]
pub struct BlockRecord {
    pub id: i64,
    pub timestamp: i64,
    pub height: u64,
    pub hash: String,
    pub difficulty: u64,
    pub reward: u64,
}

/// Daily earnings estimate.
#[derive(Debug, Clone, Serialize)]
pub struct EarningsEstimate {
    pub date: String,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub blocks_found: u64,
    pub avg_hashrate: f64,
    pub uptime_hours: f64,
}

/// SQLite database for mining history and statistics.
#[derive(Clone)]
pub struct MiningDb {
    conn: Arc<Mutex<Connection>>,
}

impl MiningDb {
    /// Open or create the database at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database: {}", path.display()))?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        info!("Database opened: {}", path.display());
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS shares (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                event_type TEXT NOT NULL,
                job_id TEXT NOT NULL DEFAULT '',
                nonce TEXT NOT NULL DEFAULT '',
                hash TEXT NOT NULL DEFAULT '',
                difficulty INTEGER NOT NULL DEFAULT 0,
                height INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS hashrate_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                hashrate REAL NOT NULL,
                threads INTEGER NOT NULL DEFAULT 0,
                accepted INTEGER NOT NULL DEFAULT 0,
                rejected INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS blocks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
                height INTEGER NOT NULL,
                hash TEXT NOT NULL,
                difficulty INTEGER NOT NULL DEFAULT 0,
                reward INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_shares_timestamp ON shares(timestamp);
            CREATE INDEX IF NOT EXISTS idx_shares_type ON shares(event_type);
            CREATE INDEX IF NOT EXISTS idx_hashrate_timestamp ON hashrate_log(timestamp);
            CREATE INDEX IF NOT EXISTS idx_blocks_height ON blocks(height);
            "
        )?;
        Ok(())
    }

    /// Record a share event (accepted, rejected, or block).
    pub fn record_share(
        &self,
        event_type: &str,
        job_id: &str,
        nonce: &str,
        hash: &str,
        difficulty: u64,
        height: u64,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO shares (event_type, job_id, nonce, hash, difficulty, height) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![event_type, job_id, nonce, hash, difficulty, height],
        )?;
        let id = conn.last_insert_rowid();
        debug!("Recorded share: {} (id={})", event_type, id);
        Ok(id)
    }

    /// Record a hashrate snapshot.
    pub fn record_hashrate(
        &self,
        hashrate: f64,
        threads: u32,
        accepted: u64,
        rejected: u64,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO hashrate_log (hashrate, threads, accepted, rejected) VALUES (?1, ?2, ?3, ?4)",
            params![hashrate, threads, accepted, rejected],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Record a block found.
    pub fn record_block(
        &self,
        height: u64,
        hash: &str,
        difficulty: u64,
        reward: u64,
    ) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO blocks (height, hash, difficulty, reward) VALUES (?1, ?2, ?3, ?4)",
            params![height, hash, difficulty, reward],
        )?;
        let id = conn.last_insert_rowid();
        info!("★ Recorded block #{} (id={})", height, id);
        Ok(id)
    }

    /// Get recent shares.
    pub fn get_recent_shares(&self, limit: u32) -> Result<Vec<ShareRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, event_type, job_id, nonce, hash, difficulty, height
             FROM shares ORDER BY id DESC LIMIT ?1"
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            Ok(ShareRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                event_type: row.get(2)?,
                job_id: row.get(3)?,
                nonce: row.get(4)?,
                hash: row.get(5)?,
                difficulty: row.get(6)?,
                height: row.get(7)?,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// Get hashrate history for charts.
    pub fn get_hashrate_history(&self, limit: u32) -> Result<Vec<HashrateSnapshot>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, hashrate, threads, accepted, rejected
             FROM hashrate_log ORDER BY id DESC LIMIT ?1"
        )?;

        let rows = stmt.query_map(params![limit], |row| {
            Ok(HashrateSnapshot {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                hashrate: row.get(2)?,
                threads: row.get(3)?,
                accepted: row.get(4)?,
                rejected: row.get(5)?,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        records.reverse(); // Oldest first for charts
        Ok(records)
    }

    /// Get all blocks found.
    pub fn get_blocks(&self) -> Result<Vec<BlockRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, timestamp, height, hash, difficulty, reward FROM blocks ORDER BY height DESC"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(BlockRecord {
                id: row.get(0)?,
                timestamp: row.get(1)?,
                height: row.get(2)?,
                hash: row.get(3)?,
                difficulty: row.get(4)?,
                reward: row.get(5)?,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// Get daily earnings estimate.
    pub fn get_daily_earnings(&self, days: u32) -> Result<Vec<EarningsEstimate>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "
            SELECT
                date(timestamp, 'unixepoch') as day,
                SUM(CASE WHEN event_type = 'accepted' THEN 1 ELSE 0 END) as accepted,
                SUM(CASE WHEN event_type = 'rejected' THEN 1 ELSE 0 END) as rejected,
                SUM(CASE WHEN event_type = 'block' THEN 1 ELSE 0 END) as blocks
            FROM shares
            WHERE timestamp > strftime('%s', 'now') - ?1 * 86400
            GROUP BY day
            ORDER BY day DESC
            "
        )?;

        let rows = stmt.query_map(params![days], |row| {
            Ok(EarningsEstimate {
                date: row.get(0)?,
                shares_accepted: row.get(1)?,
                shares_rejected: row.get(2)?,
                blocks_found: row.get(3)?,
                avg_hashrate: 0.0, // Computed separately
                uptime_hours: 0.0,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// Get summary statistics.
    pub fn get_summary(&self) -> Result<DbSummary> {
        let conn = self.conn.lock().unwrap();

        let total_shares: u64 = conn.query_row(
            "SELECT COUNT(*) FROM shares WHERE event_type = 'accepted'", [],
            |r| r.get(0),
        ).unwrap_or(0);

        let total_rejected: u64 = conn.query_row(
            "SELECT COUNT(*) FROM shares WHERE event_type = 'rejected'", [],
            |r| r.get(0),
        ).unwrap_or(0);

        let total_blocks: u64 = conn.query_row(
            "SELECT COUNT(*) FROM blocks", [],
            |r| r.get(0),
        ).unwrap_or(0);

        let total_reward: u64 = conn.query_row(
            "SELECT COALESCE(SUM(reward), 0) FROM blocks", [],
            |r| r.get(0),
        ).unwrap_or(0);

        let avg_hashrate: f64 = conn.query_row(
            "SELECT COALESCE(AVG(hashrate), 0.0) FROM hashrate_log", [],
            |r| r.get(0),
        ).unwrap_or(0.0);

        let first_share: Option<i64> = conn.query_row(
            "SELECT MIN(timestamp) FROM shares", [],
            |r| r.get(0),
        ).unwrap_or(None);

        Ok(DbSummary {
            total_accepted: total_shares,
            total_rejected,
            total_blocks,
            total_reward,
            avg_hashrate,
            first_share_timestamp: first_share,
        })
    }
}

/// Summary statistics from the database.
#[derive(Debug, Clone, Serialize)]
pub struct DbSummary {
    pub total_accepted: u64,
    pub total_rejected: u64,
    pub total_blocks: u64,
    pub total_reward: u64,
    pub avg_hashrate: f64,
    pub first_share_timestamp: Option<i64>,
}