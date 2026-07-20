use std::process::ExitCode;

use fl_client_daemon::config::Configuration;
use prettytable::{row, Table};

use crate::output;

pub async fn run(cfg: &Configuration) -> ExitCode {
    let cert_exists = cfg.certificates.cert_path.exists();
    let cert_status = if cert_exists { "✓ Found" } else { "⚠ NOT FOUND" };

    let mut t = Table::new();
    t.add_row(row!["Organization ID", cfg.organization_id]);
    t.add_row(row!["Coordinator",     cfg.coordinator.base_url]);
    t.add_row(row!["Cert Path",       cfg.certificates.cert_path.display()]);
    t.add_row(row!["Cert Status",     cert_status]);
    t.printstd();

    if !cert_exists {
        output::warn("Cert file not found — mTLS commands will fail until a valid cert is placed at the configured path.");
    }
    ExitCode::SUCCESS
}
