//! Codebase symbol indexing and fast lookup.
//!
//! Provides a lightweight regex-based AST symbol extractor and cache
//! for fast identifier lookups (functions, structs, classes, enums).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeSymbol {
    pub name: String,
    pub kind: String, // "fn", "struct", "class", "enum", "trait", "interface"
    pub file_path: String,
    pub line_number: usize,
}

#[derive(Debug, Default)]
pub struct SymbolIndex {
    symbols: Arc<Mutex<HashMap<String, Vec<CodeSymbol>>>>,
}

impl SymbolIndex {
    pub fn new() -> Self {
        Self {
            symbols: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn index_file(&self, _root: &Path, rel_path: &Path, content: &str) {
        let path_str = rel_path.to_string_lossy().to_string();
        let mut file_symbols = Vec::new();

        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if let Some(symbol) = parse_line_symbol(trimmed, &path_str, line_idx + 1) {
                file_symbols.push(symbol);
            }
        }

        if !file_symbols.is_empty() {
            let mut guard = self.symbols.lock().unwrap();
            for sym in file_symbols {
                guard.entry(sym.name.to_lowercase()).or_default().push(sym);
            }
        }
    }

    pub fn search(&self, query: &str) -> Vec<CodeSymbol> {
        let guard = self.symbols.lock().unwrap();
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for (name, syms) in guard.iter() {
            if name.contains(&query_lower) {
                results.extend(syms.clone());
            }
        }
        results.truncate(50);
        results
    }
}

fn parse_line_symbol(line: &str, file_path: &str, line_number: usize) -> Option<CodeSymbol> {
    if line.starts_with("//") || line.starts_with('#') || line.starts_with("/*") {
        return None;
    }

    let kinds = [
        ("fn ", "fn"),
        ("pub fn ", "fn"),
        ("pub async fn ", "fn"),
        ("async fn ", "fn"),
        ("struct ", "struct"),
        ("pub struct ", "struct"),
        ("enum ", "enum"),
        ("pub enum ", "enum"),
        ("trait ", "trait"),
        ("pub trait ", "trait"),
        ("class ", "class"),
        ("interface ", "interface"),
        ("def ", "fn"),
        ("function ", "fn"),
    ];

    for (prefix, kind) in kinds {
        if let Some(rest) = line.strip_prefix(prefix) {
            let name = rest
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .next()?
                .trim();
            if !name.is_empty() {
                return Some(CodeSymbol {
                    name: name.to_string(),
                    kind: kind.to_string(),
                    file_path: file_path.to_string(),
                    line_number,
                });
            }
        }
    }
    None
}
