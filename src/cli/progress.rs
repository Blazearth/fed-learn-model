use indicatif::{ProgressBar, ProgressStyle};

pub fn download_bar(total_bytes: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner} [{bar:40}] {bytes}/{total_bytes} ({eta})")
            .expect("valid template")
            .progress_chars("█▉▊▋▌▍▎▏ "),
    );
    pb
}

pub fn training_bar(total_epochs: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_epochs);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner} Training epoch {pos}/{len} [{bar:30}] {elapsed}")
            .expect("valid template"),
    );
    pb
}

pub fn upload_bar(total_bytes: u64) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner} [{bar:40}] {bytes}/{total_bytes} ({bytes_per_sec})")
            .expect("valid template"),
    );
    pb
}
