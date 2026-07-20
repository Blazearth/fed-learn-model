use std::process::ExitCode;

pub fn run() -> ExitCode {
    println!("{}", env!("CARGO_PKG_VERSION"));
    ExitCode::SUCCESS
}
