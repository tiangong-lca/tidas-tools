use std::path::Path;

use tidas_assets::{check_filesystem_lock, write_lock};

const REPO_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn main() {
    let action = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "check".to_owned());
    let root = Path::new(REPO_ROOT);
    let result = match action.as_str() {
        "check" => check_filesystem_lock(root).map(|()| {
            println!("asset lock is current");
        }),
        "write" => write_lock(root).map(|path| {
            println!("wrote {}", path.display());
        }),
        _ => {
            eprintln!("usage: tidas-asset-lock [check|write]");
            std::process::exit(64);
        }
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
