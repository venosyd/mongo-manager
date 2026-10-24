//! Configuração persistente: conexões salvas, histórico de consultas,
//! pipelines salvos e preferências. Gravada em `~/.config/mongo-manager/config.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: String,
    pub name: String,
    pub uri: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub filter: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub sort: String,
    #[serde(default)]
    pub favorite: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedPipeline {
    pub name: String,
    pub pipeline: String,
}

/// Larguras e colunas ocultas da tabela de uma coleção.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnPrefs {
    pub widths: HashMap<String, f32>,
    pub hidden: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub dark_mode: bool,
    pub page_size: u64,
    pub zoom: f32,
    pub max_time_ms: u64,
    pub sample_size: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            dark_mode: true,
            page_size: 50,
            zoom: 1.0,
            max_time_ms: 60_000,
            sample_size: 500,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub connections: Vec<SavedConnection>,
    /// Chave: `conexão/db.coleção`.
    pub history: HashMap<String, Vec<HistoryEntry>>,
    pub pipelines: HashMap<String, Vec<SavedPipeline>>,
    pub columns: HashMap<String, ColumnPrefs>,
    pub settings: Settings,
}

const MAX_HISTORY: usize = 50;

impl Config {
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("MM_CONFIG") {
            return PathBuf::from(p);
        }
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mongo-manager")
            .join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::path();
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                eprintln!("config inválida em {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            // As URIs podem conter senhas.
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    pub fn push_history(&mut self, ns: &str, entry: HistoryEntry) {
        if entry.filter.trim().is_empty() && entry.project.trim().is_empty() && entry.sort.trim().is_empty() {
            return;
        }
        let list = self.history.entry(ns.to_string()).or_default();
        let mut entry = entry;
        if let Some(pos) = list.iter().position(|e| {
            e.filter == entry.filter && e.project == entry.project && e.sort == entry.sort
        }) {
            entry.favorite = list[pos].favorite;
            list.remove(pos);
        }
        list.insert(0, entry);
        // Favoritos nunca são descartados.
        while list.len() > MAX_HISTORY {
            match list.iter().rposition(|e| !e.favorite) {
                Some(i) => {
                    list.remove(i);
                }
                None => break,
            }
        }
    }
}
