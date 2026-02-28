use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AurHelperMode {
    Auto,
    Paru,
    Yay,
    None,
}

impl Default for AurHelperMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    pub poll_minutes: u64,
    pub notify_on_change: bool,
    pub enable_aur: bool,
    pub terminal: String,
    pub official_check_cmd: String,
    pub aur_helper: AurHelperMode,
    pub upgrade_cmd: String,
}

impl Default for EffectiveConfig {
    fn default() -> Self {
        Self {
            poll_minutes: 30,
            notify_on_change: true,
            enable_aur: true,
            terminal: "auto".to_string(),
            official_check_cmd: "auto".to_string(),
            aur_helper: AurHelperMode::Auto,
            upgrade_cmd: "auto".to_string(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub poll_minutes: Option<u64>,
    pub no_aur: bool,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    poll_minutes: Option<u64>,
    notify_on_change: Option<bool>,
    enable_aur: Option<bool>,
    terminal: Option<String>,
    official_check_cmd: Option<String>,
    aur_helper: Option<AurHelperMode>,
    upgrade_cmd: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config at {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse config at {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

pub fn default_config_path() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("pactrack").join("config.toml")
}

pub fn load_config(
    path_override: Option<PathBuf>,
    cli: &CliOverrides,
) -> Result<(EffectiveConfig, PathBuf), ConfigError> {
    let path = path_override.unwrap_or_else(default_config_path);
    let from_file = read_file_config(&path)?;

    let mut merged = EffectiveConfig::default();

    if let Some(v) = from_file.poll_minutes {
        merged.poll_minutes = v.max(1);
    }
    if let Some(v) = from_file.notify_on_change {
        merged.notify_on_change = v;
    }
    if let Some(v) = from_file.enable_aur {
        merged.enable_aur = v;
    }
    if let Some(v) = from_file.terminal {
        merged.terminal = v;
    }
    if let Some(v) = from_file.official_check_cmd {
        merged.official_check_cmd = v;
    }
    if let Some(v) = from_file.aur_helper {
        merged.aur_helper = v;
    }
    if let Some(v) = from_file.upgrade_cmd {
        merged.upgrade_cmd = v;
    }

    if let Some(v) = cli.poll_minutes {
        merged.poll_minutes = v.max(1);
    }
    if cli.no_aur {
        merged.enable_aur = false;
    }

    Ok((merged, path))
}

fn read_file_config(path: &Path) -> Result<FileConfig, ConfigError> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }

    let raw = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    toml::from_str(&raw).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn config_merging_precedence_defaults_file_cli() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cfg_path = temp.path().join("config.toml");

        fs::write(
            &cfg_path,
            "poll_minutes = 45\nenable_aur = true\nnotify_on_change = false\naur_helper = \"paru\"\n",
        )
        .expect("write config");

        let cli = CliOverrides {
            poll_minutes: Some(5),
            no_aur: true,
        };

        let (cfg, _) = load_config(Some(cfg_path), &cli).expect("load config");
        assert_eq!(cfg.poll_minutes, 5);
        assert!(!cfg.enable_aur);
        assert!(!cfg.notify_on_change);
        assert_eq!(cfg.aur_helper, AurHelperMode::Paru);
    }

    #[test]
    fn missing_file_uses_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let cfg_path = temp.path().join("missing.toml");
        let (cfg, _) = load_config(Some(cfg_path), &CliOverrides::default()).expect("load");

        assert_eq!(cfg.poll_minutes, 30);
        assert!(cfg.notify_on_change);
        assert!(cfg.enable_aur);
        assert_eq!(cfg.aur_helper, AurHelperMode::Auto);
    }
}
