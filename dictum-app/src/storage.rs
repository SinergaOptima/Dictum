use std::path::{Path, PathBuf};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rand::RngCore;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const HISTORY_PAGE_SCAN_BATCH: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub created_at: String,
    pub text: String,
    pub source: String,
    pub latency_ms: i64,
    pub word_count: usize,
    pub char_count: usize,
    pub dictionary_applied: bool,
    pub snippet_applied: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub items: Vec<HistoryItem>,
    pub total: usize,
    pub page: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsBucket {
    pub date: String,
    pub utterances: usize,
    pub words: usize,
    pub chars: usize,
    pub avg_latency_ms: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsPayload {
    pub range_days: usize,
    pub total_utterances: usize,
    pub total_words: usize,
    pub total_chars: usize,
    pub avg_latency_ms: f32,
    pub buckets: Vec<StatsBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub id: String,
    pub term: String,
    pub aliases: Vec<String>,
    pub language: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetEntry {
    pub id: String,
    pub trigger: String,
    pub expansion: String,
    /// "slash" | "phrase"
    pub mode: String,
    pub apply_modes: Vec<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettings {
    pub history_enabled: bool,
    pub retention_days: usize,
    pub cloud_opt_in: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStorageSummary {
    pub db_path: String,
    pub total_records: usize,
    pub oldest_created_at: Option<String>,
    pub newest_created_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HistoryRecordInput {
    pub text: String,
    pub source: String,
    pub latency_ms: i64,
    pub dictionary_applied: bool,
    pub snippet_applied: bool,
}

#[derive(Debug, Clone)]
pub struct LocalStore {
    db_path: PathBuf,
    cipher: TextCipher,
}

#[derive(Debug, Clone)]
struct TextCipher {
    key: [u8; 32],
}

impl TextCipher {
    fn new(scope: &Path) -> Self {
        let username = std::env::var("USERNAME").unwrap_or_default();
        let computer = std::env::var("COMPUTERNAME").unwrap_or_default();
        let material = format!(
            "{username}|{computer}|{}|dictum-history-v1",
            scope.to_string_lossy()
        );
        let mut hasher = Sha256::new();
        hasher.update(material.as_bytes());
        let digest = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&digest[..32]);
        Self { key }
    }

    fn encrypt(&self, plain: &str) -> Result<String, String> {
        if plain.is_empty() {
            return Ok(String::new());
        }
        let cipher = Aes256Gcm::new_from_slice(&self.key).map_err(|e| e.to_string())?;
        let mut nonce_bytes = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let encrypted = cipher
            .encrypt(nonce, plain.as_bytes())
            .map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(12 + encrypted.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&encrypted);
        Ok(BASE64.encode(out))
    }

    fn decrypt(&self, encoded: &str) -> Option<String> {
        if encoded.is_empty() {
            return Some(String::new());
        }
        let bytes = BASE64.decode(encoded).ok()?;
        if bytes.len() <= 12 {
            return None;
        }
        let (nonce_bytes, cipher_bytes) = bytes.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let cipher = Aes256Gcm::new_from_slice(&self.key).ok()?;
        let plain = cipher.decrypt(nonce, cipher_bytes).ok()?;
        String::from_utf8(plain).ok()
    }
}

impl LocalStore {
    pub fn default_db_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Lattice Labs")
                .join("Dictum")
                .join("dictum.db")
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| PathBuf::from("/tmp"))
                        .join(".local")
                        .join("share")
                })
                .join("dictum")
                .join("dictum.db")
        }
    }

    pub fn new(db_path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let store = Self {
            cipher: TextCipher::new(&db_path),
            db_path,
        };
        store.init_schema()?;
        Ok(store)
    }

    fn open(&self) -> Result<Connection, String> {
        Connection::open(&self.db_path).map_err(|e| e.to_string())
    }

    fn init_schema(&self) -> Result<(), String> {
        let conn = self.open()?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            CREATE TABLE IF NOT EXISTS dictation_history (
              id TEXT PRIMARY KEY,
              created_at INTEGER NOT NULL,
              text_enc TEXT NOT NULL,
              source TEXT NOT NULL,
              latency_ms INTEGER NOT NULL DEFAULT 0,
              word_count INTEGER NOT NULL DEFAULT 0,
              char_count INTEGER NOT NULL DEFAULT 0,
              dictionary_applied INTEGER NOT NULL DEFAULT 0,
              snippet_applied INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS dictionary_entries (
              id TEXT PRIMARY KEY,
              term TEXT NOT NULL,
              aliases_json TEXT NOT NULL,
              language TEXT,
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS snippets (
              id TEXT PRIMARY KEY,
              trigger TEXT NOT NULL UNIQUE,
              expansion TEXT NOT NULL,
              mode TEXT NOT NULL,
              apply_modes_json TEXT NOT NULL DEFAULT '[]',
              enabled INTEGER NOT NULL DEFAULT 1,
              created_at INTEGER NOT NULL,
              updated_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_history_created_at ON dictation_history(created_at DESC);
            CREATE INDEX IF NOT EXISTS idx_dictionary_term ON dictionary_entries(term);
            CREATE INDEX IF NOT EXISTS idx_snippets_trigger ON snippets(trigger);
            "#,
        )
        .map_err(|e| e.to_string())?;
        ensure_text_column_with_default(&conn, "snippets", "apply_modes_json", "'[]'")?;
        Ok(())
    }

    pub fn prune_history(&self, retention_days: usize) -> Result<usize, String> {
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = Utc::now() - Duration::days(retention_days as i64);
        let conn = self.open()?;
        let changed = conn
            .execute(
                "DELETE FROM dictation_history WHERE created_at < ?1",
                params![cutoff.timestamp()],
            )
            .map_err(|e| e.to_string())?;
        Ok(changed)
    }

    pub fn insert_history(&self, input: HistoryRecordInput) -> Result<(), String> {
        let now = Utc::now().timestamp();
        let id = new_id("hist");
        let text_enc = self.cipher.encrypt(&input.text)?;
        let word_count = input.text.split_whitespace().count();
        let char_count = input.text.chars().count();
        let conn = self.open()?;
        conn.execute(
            r#"
            INSERT INTO dictation_history
            (id, created_at, text_enc, source, latency_ms, word_count, char_count, dictionary_applied, snippet_applied)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                id,
                now,
                text_enc,
                input.source,
                input.latency_ms,
                word_count as i64,
                char_count as i64,
                if input.dictionary_applied { 1_i64 } else { 0_i64 },
                if input.snippet_applied { 1_i64 } else { 0_i64 }
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_history(
        &self,
        page: usize,
        page_size: usize,
        query: Option<String>,
    ) -> Result<HistoryPage, String> {
        let page = page.max(1);
        let page_size = page_size.clamp(1, 200);
        let start = (page - 1).saturating_mul(page_size);
        let conn = self.open()?;
        let query = query
            .as_ref()
            .map(|q| q.trim().to_ascii_lowercase())
            .filter(|q| !q.is_empty());

        if query.is_none() {
            let total = conn
                .query_row("SELECT COUNT(*) FROM dictation_history", [], |row| {
                    row.get::<_, i64>(0)
                })
                .map_err(|e| e.to_string())? as usize;
            let mut stmt = conn
                .prepare(
                    "SELECT id, created_at, text_enc, source, latency_ms, word_count, char_count, dictionary_applied, snippet_applied
                     FROM dictation_history
                     ORDER BY created_at DESC
                     LIMIT ?1 OFFSET ?2",
                )
                .map_err(|e| e.to_string())?;
            let mut rows = stmt
                .query(params![page_size as i64, start as i64])
                .map_err(|e| e.to_string())?;
            let mut items = Vec::new();
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                if let Some(item) = self.read_history_row(row)? {
                    items.push(item);
                }
            }

            return Ok(HistoryPage {
                items,
                total,
                page,
                page_size,
            });
        }

        let query = query.expect("query checked above");
        let mut offset = 0usize;
        let mut total = 0usize;
        let mut items = Vec::new();
        let end = start.saturating_add(page_size);

        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, text_enc, source, latency_ms, word_count, char_count, dictionary_applied, snippet_applied
                 FROM dictation_history
                 ORDER BY created_at DESC
                 LIMIT ?1 OFFSET ?2",
            )
            .map_err(|e| e.to_string())?;

        loop {
            let mut rows = stmt
                .query(params![HISTORY_PAGE_SCAN_BATCH as i64, offset as i64])
                .map_err(|e| e.to_string())?;
            let mut scanned = 0usize;

            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                scanned += 1;
                let Some(item) = self.read_history_row(row)? else {
                    continue;
                };
                if !item.text.to_ascii_lowercase().contains(&query) {
                    continue;
                }

                if total >= start && total < end {
                    items.push(item);
                }
                total += 1;
            }

            if scanned < HISTORY_PAGE_SCAN_BATCH {
                break;
            }
            offset = offset.saturating_add(HISTORY_PAGE_SCAN_BATCH);
        }

        Ok(HistoryPage {
            items,
            total,
            page,
            page_size,
        })
    }

    pub fn delete_history(
        &self,
        ids: Option<Vec<String>>,
        older_than_days: Option<usize>,
    ) -> Result<usize, String> {
        let conn = self.open()?;
        let mut deleted = 0usize;

        if let Some(ids) = ids {
            for id in ids {
                deleted += conn
                    .execute("DELETE FROM dictation_history WHERE id = ?1", params![id])
                    .map_err(|e| e.to_string())?;
            }
        }

        if let Some(days) = older_than_days {
            let cutoff = Utc::now() - Duration::days(days as i64);
            deleted += conn
                .execute(
                    "DELETE FROM dictation_history WHERE created_at < ?1",
                    params![cutoff.timestamp()],
                )
                .map_err(|e| e.to_string())?;
        }

        Ok(deleted)
    }

    pub fn get_stats(&self, range_days: usize) -> Result<StatsPayload, String> {
        let range_days = range_days.clamp(1, 365);
        let cutoff = Utc::now() - Duration::days(range_days as i64);
        let conn = self.open()?;
        let totals = conn
            .query_row(
                "SELECT
                    COUNT(*),
                    COALESCE(SUM(word_count), 0),
                    COALESCE(SUM(char_count), 0),
                    COALESCE(AVG(CASE WHEN latency_ms > 0 THEN latency_ms END), 0.0)
                 FROM dictation_history
                 WHERE created_at >= ?1",
                params![cutoff.timestamp()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as usize,
                        row.get::<_, i64>(1)? as usize,
                        row.get::<_, i64>(2)? as usize,
                        row.get::<_, f64>(3)? as f32,
                    ))
                },
            )
            .map_err(|e| e.to_string())?;

        let mut stmt = conn
            .prepare(
                "SELECT
                    date(created_at, 'unixepoch') AS day,
                    COUNT(*) AS utterances,
                    COALESCE(SUM(word_count), 0) AS words,
                    COALESCE(SUM(char_count), 0) AS chars,
                    COALESCE(AVG(CASE WHEN latency_ms > 0 THEN latency_ms END), 0.0) AS avg_latency_ms
                 FROM dictation_history
                 WHERE created_at >= ?1
                 GROUP BY day
                 ORDER BY day ASC",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params![cutoff.timestamp()])
            .map_err(|e| e.to_string())?;

        let mut out_buckets = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            out_buckets.push(StatsBucket {
                date: row.get(0).map_err(|e| e.to_string())?,
                utterances: row.get::<_, i64>(1).map_err(|e| e.to_string())? as usize,
                words: row.get::<_, i64>(2).map_err(|e| e.to_string())? as usize,
                chars: row.get::<_, i64>(3).map_err(|e| e.to_string())? as usize,
                avg_latency_ms: row.get::<_, f64>(4).map_err(|e| e.to_string())? as f32,
            });
        }

        Ok(StatsPayload {
            range_days,
            total_utterances: totals.0,
            total_words: totals.1,
            total_chars: totals.2,
            avg_latency_ms: totals.3,
            buckets: out_buckets,
        })
    }

    pub fn history_storage_summary(&self) -> Result<HistoryStorageSummary, String> {
        let conn = self.open()?;
        let (total_records, oldest, newest) = conn
            .query_row(
                "SELECT COUNT(*), MIN(created_at), MAX(created_at) FROM dictation_history",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)? as usize,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .map_err(|e| e.to_string())?;

        Ok(HistoryStorageSummary {
            db_path: self.db_path.display().to_string(),
            total_records,
            oldest_created_at: oldest.map(ts_to_rfc3339),
            newest_created_at: newest.map(ts_to_rfc3339),
        })
    }

    pub fn list_dictionary(&self) -> Result<Vec<DictionaryEntry>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, term, aliases_json, language, enabled, created_at, updated_at
                 FROM dictionary_entries ORDER BY updated_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let aliases_json: String = row.get(2).map_err(|e| e.to_string())?;
            let aliases = serde_json::from_str::<Vec<String>>(&aliases_json).unwrap_or_default();
            let created_at = ts_to_rfc3339(row.get::<_, i64>(5).map_err(|e| e.to_string())?);
            let updated_at = ts_to_rfc3339(row.get::<_, i64>(6).map_err(|e| e.to_string())?);
            out.push(DictionaryEntry {
                id: row.get(0).map_err(|e| e.to_string())?,
                term: row.get(1).map_err(|e| e.to_string())?,
                aliases,
                language: row.get(3).map_err(|e| e.to_string())?,
                enabled: row.get::<_, i64>(4).map_err(|e| e.to_string())? != 0,
                created_at,
                updated_at,
            });
        }
        Ok(out)
    }

    pub fn upsert_dictionary(&self, mut entry: DictionaryEntry) -> Result<DictionaryEntry, String> {
        let now = Utc::now().timestamp();
        if entry.id.trim().is_empty() {
            entry.id = new_id("dict");
            entry.created_at = ts_to_rfc3339(now);
        }
        entry.updated_at = ts_to_rfc3339(now);
        let aliases_json = serde_json::to_string(&entry.aliases).map_err(|e| e.to_string())?;
        let conn = self.open()?;
        conn.execute(
            r#"
            INSERT INTO dictionary_entries (id, term, aliases_json, language, enabled, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, COALESCE((SELECT created_at FROM dictionary_entries WHERE id = ?1), ?6), ?7)
            ON CONFLICT(id) DO UPDATE SET
                term = excluded.term,
                aliases_json = excluded.aliases_json,
                language = excluded.language,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
            params![
                entry.id,
                entry.term.trim(),
                aliases_json,
                entry.language,
                if entry.enabled { 1_i64 } else { 0_i64 },
                now,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    pub fn delete_dictionary(&self, id: &str) -> Result<(), String> {
        let conn = self.open()?;
        conn.execute("DELETE FROM dictionary_entries WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list_snippets(&self) -> Result<Vec<SnippetEntry>, String> {
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, trigger, expansion, mode, apply_modes_json, enabled, created_at, updated_at
                 FROM snippets ORDER BY updated_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let apply_modes_json: String = row.get(4).map_err(|e| e.to_string())?;
            let apply_modes = normalize_apply_modes(
                &serde_json::from_str::<Vec<String>>(&apply_modes_json).unwrap_or_default(),
            );
            out.push(SnippetEntry {
                id: row.get(0).map_err(|e| e.to_string())?,
                trigger: row.get(1).map_err(|e| e.to_string())?,
                expansion: row.get(2).map_err(|e| e.to_string())?,
                mode: row.get(3).map_err(|e| e.to_string())?,
                apply_modes,
                enabled: row.get::<_, i64>(5).map_err(|e| e.to_string())? != 0,
                created_at: ts_to_rfc3339(row.get::<_, i64>(6).map_err(|e| e.to_string())?),
                updated_at: ts_to_rfc3339(row.get::<_, i64>(7).map_err(|e| e.to_string())?),
            });
        }
        Ok(out)
    }

    pub fn upsert_snippet(&self, mut entry: SnippetEntry) -> Result<SnippetEntry, String> {
        let now = Utc::now().timestamp();
        if entry.id.trim().is_empty() {
            entry.id = new_id("snip");
            entry.created_at = ts_to_rfc3339(now);
        }
        entry.updated_at = ts_to_rfc3339(now);
        let mode = match entry.mode.as_str() {
            "phrase" => "phrase",
            _ => "slash",
        };
        entry.mode = mode.to_string();
        entry.apply_modes = normalize_apply_modes(&entry.apply_modes);
        let apply_modes_json =
            serde_json::to_string(&entry.apply_modes).map_err(|e| e.to_string())?;

        let conn = self.open()?;
        conn.execute(
            r#"
            INSERT INTO snippets (id, trigger, expansion, mode, apply_modes_json, enabled, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE((SELECT created_at FROM snippets WHERE id = ?1), ?7), ?8)
            ON CONFLICT(id) DO UPDATE SET
                trigger = excluded.trigger,
                expansion = excluded.expansion,
                mode = excluded.mode,
                apply_modes_json = excluded.apply_modes_json,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
            params![
                entry.id,
                entry.trigger.trim(),
                entry.expansion.trim(),
                entry.mode,
                apply_modes_json,
                if entry.enabled { 1_i64 } else { 0_i64 },
                now,
                now,
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(entry)
    }

    pub fn delete_snippet(&self, id: &str) -> Result<(), String> {
        let conn = self.open()?;
        conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn read_history_row(&self, row: &rusqlite::Row<'_>) -> Result<Option<HistoryItem>, String> {
        let enc: String = row.get(2).map_err(|e| e.to_string())?;
        let Some(text) = self.cipher.decrypt(&enc) else {
            return Ok(None);
        };
        let created_at: i64 = row.get(1).map_err(|e| e.to_string())?;
        Ok(Some(HistoryItem {
            id: row.get(0).map_err(|e| e.to_string())?,
            created_at: Utc
                .timestamp_opt(created_at, 0)
                .single()
                .unwrap_or_else(Utc::now)
                .to_rfc3339(),
            text,
            source: row.get(3).map_err(|e| e.to_string())?,
            latency_ms: row.get(4).map_err(|e| e.to_string())?,
            word_count: row.get::<_, i64>(5).map_err(|e| e.to_string())? as usize,
            char_count: row.get::<_, i64>(6).map_err(|e| e.to_string())? as usize,
            dictionary_applied: row.get::<_, i64>(7).map_err(|e| e.to_string())? != 0,
            snippet_applied: row.get::<_, i64>(8).map_err(|e| e.to_string())? != 0,
        }))
    }
}

fn ts_to_rfc3339(ts: i64) -> String {
    let dt: DateTime<Utc> = Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now);
    dt.to_rfc3339()
}

fn new_id(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{:08x}",
        Utc::now().timestamp_micros(),
        rand::random::<u32>()
    )
}

fn ensure_text_column_with_default(
    conn: &Connection,
    table: &str,
    column: &str,
    default_sql: &str,
) -> Result<(), String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let existing: String = row.get(1).map_err(|e| e.to_string())?;
        if existing == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} TEXT NOT NULL DEFAULT {default_sql}"),
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn normalize_apply_modes(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for mode in raw {
        let normalized = match mode.trim().to_ascii_lowercase().as_str() {
            "coding" | "code" => "coding",
            "command" | "commands" => "command",
            "conversation" | "general" | "default" => "conversation",
            _ => continue,
        };
        if !out.iter().any(|entry: &String| entry == normalized) {
            out.push(normalized.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{new_id, LocalStore};
    use chrono::Utc;
    use rusqlite::params;
    use std::path::PathBuf;

    fn temp_db_path(test_name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "dictum-storage-test-{test_name}-{}-{}.db",
            std::process::id(),
            rand::random::<u32>()
        ));
        path
    }

    fn seed_history(store: &LocalStore, text: &str, latency_ms: i64, created_at: i64) {
        let conn = store.open().expect("open store");
        let text_enc = store.cipher.encrypt(text).expect("encrypt");
        let word_count = text.split_whitespace().count() as i64;
        let char_count = text.chars().count() as i64;
        conn.execute(
            r#"
            INSERT INTO dictation_history
            (id, created_at, text_enc, source, latency_ms, word_count, char_count, dictionary_applied, snippet_applied)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0)
            "#,
            params![
                new_id("hist"),
                created_at,
                text_enc,
                "local",
                latency_ms,
                word_count,
                char_count
            ],
        )
            .expect("insert history");
    }

    #[test]
    fn get_history_pages_without_query_in_sql_order() {
        let db_path = temp_db_path("history-page");
        let store = LocalStore::new(db_path.clone()).expect("create store");
        let base = Utc::now().timestamp();
        seed_history(&store, "first entry", 10, base - 2);
        seed_history(&store, "second entry", 20, base - 1);
        seed_history(&store, "third entry", 30, base);

        let page = store.get_history(1, 2, None).expect("get history");
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].text, "third entry");
        assert_eq!(page.items[1].text, "second entry");

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_history_query_filters_matches_without_loading_all_items() {
        let db_path = temp_db_path("history-query");
        let store = LocalStore::new(db_path.clone()).expect("create store");
        let base = Utc::now().timestamp();
        seed_history(&store, "alpha bravo", 10, base - 2);
        seed_history(&store, "charlie delta", 20, base - 1);
        seed_history(&store, "echo alpha", 30, base);

        let page = store
            .get_history(1, 10, Some("alpha".into()))
            .expect("query history");
        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 2);
        assert!(page.items.iter().all(|item| item.text.contains("alpha")));

        let _ = std::fs::remove_file(db_path);
    }

    #[test]
    fn get_stats_aggregates_in_sql() {
        let db_path = temp_db_path("stats");
        let store = LocalStore::new(db_path.clone()).expect("create store");
        let base = Utc::now().timestamp();
        seed_history(&store, "one two", 100, base - 1);
        seed_history(&store, "three four five", 200, base);

        let stats = store.get_stats(30).expect("get stats");
        assert_eq!(stats.total_utterances, 2);
        assert_eq!(stats.total_words, 5);
        assert_eq!(
            stats.total_chars,
            "one two".chars().count() + "three four five".chars().count()
        );
        assert_eq!(stats.buckets.len(), 1);
        assert!(stats.avg_latency_ms >= 100.0);

        let _ = std::fs::remove_file(db_path);
    }
}
