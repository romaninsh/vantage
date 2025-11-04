use std::env;
use std::process;
use vantage_config::VantageConfig;

fn main() {
    let args: Vec<String> = env::args().collect();

    let config_path = if args.len() > 1 {
        &args[1]
    } else {
        "vantage-config.yaml"
    };

    match VantageConfig::from_file(config_path) {
        Ok(_config) => {
            println!("Syntax OK");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
