use std::sync::Arc;

use parking_lot::RwLock;

use crate::storage::{DictionaryEntry, LocalStore, SnippetEntry};

#[derive(Debug, Clone)]
pub struct TransformResult {
    pub text: String,
    pub dictionary_applied: bool,
    pub snippet_applied: bool,
}

#[derive(Debug, Clone, Default)]
struct TransformCache {
    dictionary: Vec<DictionaryEntry>,
    snippets: Vec<SnippetEntry>,
}

#[derive(Clone)]
pub struct TextTransform {
    cache: Arc<RwLock<TransformCache>>,
    store: Arc<LocalStore>,
}

impl TextTransform {
    pub fn new(store: Arc<LocalStore>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(TransformCache::default())),
            store,
        }
    }

    pub fn refresh(&self) -> Result<(), String> {
        let dictionary = self.store.list_dictionary()?;
        let snippets = self.store.list_snippets()?;
        let mut guard = self.cache.write();
        guard.dictionary = dictionary;
        guard.snippets = snippets;
        Ok(())
    }

    pub fn apply(&self, text: &str) -> TransformResult {
        let guard = self.cache.read();
        let mut out = text.trim().to_string();
        if out.is_empty() {
            return TransformResult {
                text: out,
                dictionary_applied: false,
                snippet_applied: false,
            };
        }

        let mut dictionary_applied = false;
        for entry in guard.dictionary.iter().filter(|e| e.enabled) {
            let canonical = entry.term.trim();
            if canonical.is_empty() {
                continue;
            }
            for alias in entry.aliases.iter().chain(std::iter::once(&entry.term)) {
                let alias = alias.trim();
                if alias.is_empty() {
                    continue;
                }
                let replaced = replace_word_case_aware(&out, alias, canonical);
                if replaced != out {
                    dictionary_applied = true;
                    out = replaced;
                }
            }
        }

        let mut snippet_applied = false;
        for snippet in guard.snippets.iter().filter(|s| s.enabled) {
            let trigger = snippet.trigger.trim();
            let expansion = snippet.expansion.trim();
            if trigger.is_empty() || expansion.is_empty() {
                continue;
            }
            let replaced = match snippet.mode.as_str() {
                "phrase" => replace_word_case_insensitive(&out, trigger, expansion),
                _ => replace_slash_trigger(&out, trigger, expansion),
            };
            if replaced != out {
                snippet_applied = true;
                out = replaced;
            }
        }
        if snippet_applied {
            out = strip_terminal_period(&out);
        }

        TransformResult {
            text: out,
            dictionary_applied,
            snippet_applied,
        }
    }
}

fn replace_slash_trigger(text: &str, trigger: &str, replacement: &str) -> String {
    let with_slash = if trigger.starts_with('/') {
        trigger.to_string()
    } else {
        format!("/{trigger}")
    };
    replace_word_case_insensitive(text, &with_slash, replacement)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

fn replace_word_case_aware(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let needle_lower = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut changed = false;
    while i < chars.len() {
        let rem: String = chars[i..].iter().collect();
        if rem.to_ascii_lowercase().starts_with(&needle_lower) {
            let start_ok = if i == 0 {
                true
            } else {
                !is_word_char(chars[i - 1])
            };
            let end_idx = i + needle.chars().count();
            let end_ok = if end_idx >= chars.len() {
                true
            } else {
                !is_word_char(chars[end_idx])
            };
            if start_ok && end_ok {
                let source_slice: String = chars[i..end_idx].iter().collect();
                out.push_str(match_case(&source_slice, replacement).as_str());
                i = end_idx;
                changed = true;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if changed {
        out
    } else {
        text.to_string()
    }
}

fn replace_word_case_insensitive(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || text.is_empty() {
        return text.to_string();
    }

    let needle_lower = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut changed = false;
    while i < chars.len() {
        let rem: String = chars[i..].iter().collect();
        if rem.to_ascii_lowercase().starts_with(&needle_lower) {
            let start_ok = if i == 0 {
                true
            } else {
                !is_word_char(chars[i - 1])
            };
            let end_idx = i + needle.chars().count();
            let end_ok = if end_idx >= chars.len() {
                true
            } else {
                !is_word_char(chars[end_idx])
            };
            if start_ok && end_ok {
                out.push_str(replacement);
                i = end_idx;
                changed = true;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    if changed {
        out
    } else {
        text.to_string()
    }
}

fn strip_terminal_period(text: &str) -> String {
    let trimmed_end = text.trim_end();
    if let Some(without_period) = trimmed_end.strip_suffix('.') {
        without_period.trim_end().to_string()
    } else {
        text.to_string()
    }
}

fn match_case(source: &str, replacement: &str) -> String {
    if source.chars().all(|c| c.is_uppercase()) {
        replacement.to_ascii_uppercase()
    } else if source
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
    {
        let mut chars = replacement.chars();
        if let Some(first) = chars.next() {
            format!("{}{}", first.to_ascii_uppercase(), chars.as_str())
        } else {
            replacement.to_string()
        }
    } else {
        replacement.to_string()
    }
}
