use std::{env, fs};

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() == 2 && args[1] == "--help" {
        println!("usage: blkit SOURCE.bl OUTPUT.rs");
        return;
    }
    if args.len() != 3 {
        eprintln!("usage: blkit SOURCE.bl OUTPUT.rs");
        std::process::exit(2);
    }
    let result = fs::read_to_string(&args[1])
        .map_err(|error| error.to_string())
        .and_then(|source| blkit::transpile(&source))
        .and_then(|rust| fs::write(&args[2], rust).map_err(|error| error.to_string()));
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
