use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Default)]
pub struct SubmissionState {
    pub last_submitted_epoch: Option<u64>,
    pub submitted_at: Option<String>,
}

impl SubmissionState {
    fn path(dir: &Path) -> std::path::PathBuf {
        dir.join("submission_state.json")
    }

    pub fn load(working_dir: &Path) -> Self {
        std::fs::read_to_string(Self::path(working_dir))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&mut self, epoch: u64, working_dir: &Path) -> anyhow::Result<()> {
        self.last_submitted_epoch = Some(epoch);
        self.submitted_at = Some(chrono::Utc::now().to_rfc3339());
        let json = serde_json::to_string_pretty(self)?;
        // ponytail: atomic write via .tmp + rename — prevents state corruption on kill
        let tmp = Self::path(working_dir).with_extension("tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, Self::path(working_dir))?;
        Ok(())
    }

    pub fn is_submitted(&self, epoch: u64) -> bool {
        self.last_submitted_epoch == Some(epoch)
    }
}
