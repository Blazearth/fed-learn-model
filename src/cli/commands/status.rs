use fl_client_daemon::config::Configuration;
use prettytable::{row, Table};

use crate::state::SubmissionState;

pub fn run(cfg: &Configuration, state: &SubmissionState) {
    let cert_exists = cfg.certificates.cert_path.exists();
    let last = state.last_submitted_epoch.map(|n| n.to_string()).unwrap_or_else(|| "None".to_string());

    let mut t = Table::new();
    t.add_row(row!["Organization",         cfg.organization_id]);
    t.add_row(row!["Coordinator",          cfg.coordinator.base_url]);
    t.add_row(row!["Cert Path",            cfg.certificates.cert_path.display()]);
    t.add_row(row!["Cert Exists",          if cert_exists { "Yes" } else { "No" }]);
    t.add_row(row!["Last Submitted Epoch", last]);
    t.printstd();
    // Returns to menu — does not exit process
}
