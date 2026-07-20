use std::path::PathBuf;
use std::sync::Arc;

use fl_client_daemon::config::{ConfigManager, Configuration};

pub async fn load_config(flag: Option<PathBuf>) -> anyhow::Result<Arc<Configuration>> {
    let path = if let Some(p) = flag {
        p
    } else {
        let primary = PathBuf::from("/etc/fl-daemon/config.toml");
        if primary.exists() {
            primary
        } else {
            dirs::home_dir()
                .map(|h| h.join(".fl-client/config.toml"))
                .filter(|p| p.exists())
                .ok_or_else(|| anyhow::anyhow!(
                    "No config found at /etc/fl-daemon/config.toml or ~/.fl-client/config.toml. \
                     Run 'fl-client init' to create one."
                ))?
        }
    };
    let mgr = ConfigManager::new(path).await?;
    Ok(mgr.get())
}
