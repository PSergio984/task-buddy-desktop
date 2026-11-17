use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessRule {
    pub id: i64,
    pub process_name: String,
    pub category: String, // 'whitelist' | 'blacklist' | 'ambiguous'
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusSessionRecord {
    pub id: String,
    pub task_id: Option<i64>,
    pub task_title: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub target_duration_minutes: u32,
    pub actual_duration_seconds: u32,
    pub status: String, // 'running' | 'completed' | 'aborted'
    pub diversion_count: u32,
    pub synced_to_backend: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiversionEventRecord {
    pub id: String,
    pub session_id: String,
    pub timestamp: String,
    pub trigger_type: String,
    pub detected_title: Option<String>,
    pub bypassed: bool,
}

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn new_in_memory() -> Result<Self> {
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
            r#"
            CREATE TABLE IF NOT EXISTS local_rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                process_name TEXT NOT NULL UNIQUE,
                category TEXT NOT NULL CHECK(category IN ('whitelist', 'blacklist', 'ambiguous')),
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS focus_sessions (
                id TEXT PRIMARY KEY,
                task_id INTEGER,
                task_title TEXT NOT NULL,
                started_at DATETIME NOT NULL,
                ended_at DATETIME,
                target_duration_minutes INTEGER NOT NULL,
                actual_duration_seconds INTEGER DEFAULT 0,
                status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'aborted')),
                diversion_count INTEGER DEFAULT 0,
                synced_to_backend BOOLEAN DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS diversion_events (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES focus_sessions(id),
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
                trigger_type TEXT NOT NULL,
                detected_title TEXT,
                bypassed BOOLEAN NOT NULL DEFAULT 0
            );

            -- Default seed rules
            INSERT OR IGNORE INTO local_rules (process_name, category) VALUES
                ('code.exe', 'whitelist'),
                ('cursor.exe', 'whitelist'),
                ('windowsterminal.exe', 'whitelist'),
                ('powershell.exe', 'whitelist'),
                ('cmd.exe', 'whitelist'),
                ('steam.exe', 'blacklist'),
                ('discord.exe', 'blacklist'),
                ('spotify.exe', 'blacklist'),
                ('telegram.exe', 'blacklist');
            "#,
        )?;

        Ok(())
    }

    pub fn get_rules(&self) -> Result<Vec<ProcessRule>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, process_name, category, created_at FROM local_rules ORDER BY process_name ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(ProcessRule {
                id: row.get(0)?,
                process_name: row.get(1)?,
                category: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?;

        let mut rules = Vec::new();
        for r in rows {
            rules.push(r?);
        }
        Ok(rules)
    }

    pub fn add_rule(&self, process_name: &str, category: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let name = process_name.trim().to_lowercase();
        conn.execute(
            "INSERT INTO local_rules (process_name, category) VALUES (?1, ?2)
             ON CONFLICT(process_name) DO UPDATE SET category = excluded.category",
            params![name, category],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM local_rules WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn insert_session(&self, record: &FocusSessionRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO focus_sessions 
               (id, task_id, task_title, started_at, ended_at, target_duration_minutes, actual_duration_seconds, status, diversion_count, synced_to_backend)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            params![
                record.id,
                record.task_id,
                record.task_title,
                record.started_at,
                record.ended_at,
                record.target_duration_minutes,
                record.actual_duration_seconds,
                record.status,
                record.diversion_count,
                record.synced_to_backend
            ],
        )?;
        Ok(())
    }

    pub fn update_session_end(
        &self,
        id: &str,
        ended_at: &str,
        actual_duration_seconds: u32,
        status: &str,
        diversion_count: u32,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"UPDATE focus_sessions 
               SET ended_at = ?2, actual_duration_seconds = ?3, status = ?4, diversion_count = ?5
               WHERE id = ?1"#,
            params![id, ended_at, actual_duration_seconds, status, diversion_count],
        )?;
        Ok(())
    }

    pub fn get_recent_sessions(&self, limit: u32) -> Result<Vec<FocusSessionRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            r#"SELECT id, task_id, task_title, started_at, ended_at, target_duration_minutes, actual_duration_seconds, status, diversion_count, synced_to_backend
               FROM focus_sessions ORDER BY started_at DESC LIMIT ?1"#
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(FocusSessionRecord {
                id: row.get(0)?,
                task_id: row.get(1)?,
                task_title: row.get(2)?,
                started_at: row.get(3)?,
                ended_at: row.get(4)?,
                target_duration_minutes: row.get(5)?,
                actual_duration_seconds: row.get(6)?,
                status: row.get(7)?,
                diversion_count: row.get(8)?,
                synced_to_backend: row.get(9)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn record_diversion(&self, record: &DiversionEventRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            r#"INSERT INTO diversion_events (id, session_id, timestamp, trigger_type, detected_title, bypassed)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
            params![
                record.id,
                record.session_id,
                record.timestamp,
                record.trigger_type,
                record.detected_title,
                record.bypassed
            ],
        )?;
        Ok(())
    }
}
