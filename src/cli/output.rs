use colored::Colorize;

fn color_ok() -> bool {
    std::env::var("NO_COLOR").is_err() && atty::is(atty::Stream::Stdout)
}

pub fn success(msg: &str) {
    if color_ok() { println!("{}", msg.green()); } else { println!("{msg}"); }
}

pub fn warn(msg: &str) {
    if color_ok() { println!("{}", msg.yellow()); } else { println!("WARNING: {msg}"); }
}

pub fn error(msg: &str) {
    if color_ok() { eprintln!("{}", msg.red()); } else { eprintln!("ERROR: {msg}"); }
}
