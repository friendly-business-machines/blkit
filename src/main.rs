use std::{env, fs};

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() == 2 && args[1] == "--help" {
        println!("usage: blkit SOURCE.bl OUTPUT.rs | blkit build|update [PROJECT_DIR]");
        return;
    }
    if args
        .get(1)
        .is_some_and(|arg| matches!(arg.as_str(), "build" | "update"))
    {
        if args.len() > 3 {
            eprintln!("usage: blkit build|update [PROJECT_DIR]");
            std::process::exit(2);
        }
        let root = args.get(2).map_or(".", String::as_str);
        let result =
            blkit::project::Project::load(std::path::Path::new(root)).and_then(|project| {
                if args[1] == "build" {
                    project.build()
                } else {
                    project.update()
                }
            });
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if args.len() != 3 {
        eprintln!("usage: blkit SOURCE.bl OUTPUT.rs | blkit build|update [PROJECT_DIR]");
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
