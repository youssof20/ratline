#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args = ratline_lib::cli::parse_args(std::env::args());
    if args.version {
        ratline_lib::cli::ensure_console();
        println!("ratline {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    ratline_lib::run_with_args(args);
}
