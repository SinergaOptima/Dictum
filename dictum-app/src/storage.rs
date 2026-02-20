use std::path::{Path, PathBuf};

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, Datelike, Duration, TimeZone, Utc};
use rand::RngCore;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
        let conn = self.open()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, text_enc, source, latency_ms, word_count, char_count, dictionary_applied, snippet_applied
                 FROM dictation_history ORDER BY created_at DESC LIMIT 5000",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        let query = query
            .as_ref()
            .map(|q| q.trim().to_ascii_lowercase())
            .filter(|q| !q.is_empty());

        let mut items = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let enc: String = row.get(2).map_err(|e| e.to_string())?;
            let Some(text) = self.cipher.decrypt(&enc) else {
                continue;
            };
            if let Some(ref q) = query {
                if !text.to_ascii_lowercase().contains(q) {
                    continue;
                }
            }
            let created_at: i64 = row.get(1).map_err(|e| e.to_string())?;
            let created = Utc
                .timestamp_opt(created_at, 0)
                .single()
                .unwrap_or_else(Utc::now)
                .to_rfc3339();
            items.push(HistoryItem {
                id: row.get(0).map_err(|e| e.to_string())?,
                created_at: created,
                text,
                source: row.get(3).map_err(|e| e.to_string())?,
                latency_ms: row.get(4).map_err(|e| e.to_string())?,
                word_count: row.get::<_, i64>(5).map_err(|e| e.to_string())? as usize,
                char_count: row.get::<_, i64>(6).map_err(|e| e.to_string())? as usize,
                dictionary_applied: row.get::<_, i64>(7).map_err(|e| e.to_string())? != 0,
                snippet_applied: row.get::<_, i64>(8).map_err(|e| e.to_string())? != 0,
            });
        }

        let total = items.len();
        let start = (page - 1).saturating_mul(page_size);
        let end = (start + page_size).min(total);
        let paged = if start >= total {
            Vec::new()
        } else {
            items[start..end].to_vec()
        };

        Ok(HistoryPage {
            items: paged,
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
        let mut stmt = conn
            .prepare(
                "SELECT created_at, word_count, char_count, latency_ms
                 FROM dictation_history
                 WHERE created_at >= ?1
                 ORDER BY created_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(params![cutoff.timestamp()])
            .map_err(|e| e.to_string())?;

        #[derive(Default)]
        struct DayAgg {
            utterances: usize,
            words: usize,
            chars: usize,
            latency_total: i64,
        }

        let mut buckets: std::collections::BTreeMap<(i32, u32, u32), DayAgg> =
            std::collections::BTreeMap::new();
        let mut total_utterances = 0usize;
        let mut total_words = 0usize;
        let mut total_chars = 0usize;
        let mut latency_total = 0i64;

        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let ts: i64 = row.get(0).map_err(|e| e.to_string())?;
            let words: usize = row.get::<_, i64>(1).map_err(|e| e.to_string())? as usize;
            let chars: usize = row.get::<_, i64>(2).map_err(|e| e.to_string())? as usize;
            let latency: i64 = row.get(3).map_err(|e| e.to_string())?;
            let dt = Utc.timestamp_opt(ts, 0).single().unwrap_or_else(Utc::now);
            let key = (dt.year(), dt.month(), dt.day());
            let day = buckets.entry(key).or_default();
            day.utterances += 1;
            day.words += words;
            day.chars += chars;
            day.latency_total += latency.max(0);

            total_utterances += 1;
            total_words += words;
            total_chars += chars;
            latency_total += latency.max(0);
        }

        let mut out_buckets = Vec::with_capacity(buckets.len());
        for ((y, m, d), day) in buckets {
            out_buckets.push(StatsBucket {
                date: format!("{y:04}-{m:02}-{d:02}"),
                utterances: day.utterances,
                words: day.words,
                chars: day.chars,
                avg_latency_ms: if day.utterances == 0 {
                    0.0
                } else {
                    day.latency_total as f32 / day.utterances as f32
                },
            });
        }

        Ok(StatsPayload {
            range_days,
            total_utterances,
            total_words,
            total_chars,
            avg_latency_ms: if total_utterances == 0 {
                0.0
            } else {
                latency_total as f32 / total_utterances as f32
            },
            buckets: out_buckets,
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
                "SELECT id, trigger, expansion, mode, enabled, created_at, updated_at
                 FROM snippets ORDER BY updated_at DESC",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            out.push(SnippetEntry {
                id: row.get(0).map_err(|e| e.to_string())?,
                trigger: row.get(1).map_err(|e| e.to_string())?,
                expansion: row.get(2).map_err(|e| e.to_string())?,
                mode: row.get(3).map_err(|e| e.to_string())?,
                enabled: row.get::<_, i64>(4).map_err(|e| e.to_string())? != 0,
                created_at: ts_to_rfc3339(row.get::<_, i64>(5).map_err(|e| e.to_string())?),
                updated_at: ts_to_rfc3339(row.get::<_, i64>(6).map_err(|e| e.to_string())?),
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

        let conn = self.open()?;
        conn.execute(
            r#"
            INSERT INTO snippets (id, trigger, expansion, mode, enabled, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, COALESCE((SELECT created_at FROM snippets WHERE id = ?1), ?6), ?7)
            ON CONFLICT(id) DO UPDATE SET
                trigger = excluded.trigger,
                expansion = excluded.expansion,
                mode = excluded.mode,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
            params![
                entry.id,
                entry.trigger.trim(),
                entry.expansion.trim(),
                entry.mode,
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
