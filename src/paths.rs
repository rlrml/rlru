use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

pub const APP_ID: &str = "rlru";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let config_base = dirs::config_dir().context("failed to find the user config directory")?;
        let cache_base = dirs::cache_dir().context("failed to find the user cache directory")?;
        let data_base = dirs::data_dir().context("failed to find the user data directory")?;
        Ok(Self {
            config_dir: config_base.join(APP_ID),
            cache_dir: cache_base.join(APP_ID),
            data_dir: data_base.join(APP_ID),
        })
    }

    pub fn ensure(&self) -> Result<()> {
        create_private_dir(&self.config_dir)?;
        create_private_dir(&self.cache_dir)?;
        create_private_dir(&self.data_dir)?;
        create_private_dir(&self.tokens_dir())?;
        Ok(())
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn tokens_dir(&self) -> PathBuf {
        self.config_dir.join("tokens")
    }

    pub fn upload_cache_path(&self, target_name: &str) -> PathBuf {
        self.cache_dir.join(format!(
            "uploaded-{}.txt",
            sanitize_path_segment(target_name)
        ))
    }

    pub fn sync_state_file(&self) -> PathBuf {
        self.data_dir.join("sync-state.toml")
    }

    pub fn upload_failures_file(&self) -> PathBuf {
        self.data_dir.join("upload-failures.toml")
    }
}

fn create_private_dir(path: &PathBuf) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("failed to set private permissions on {}", path.display()))?;
    }

    Ok(())
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
