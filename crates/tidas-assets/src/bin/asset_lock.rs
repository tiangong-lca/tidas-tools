use std::path::Path;

use tidas_assets::{
    check_filesystem_lock, check_filesystem_schema_lock, write_lock, write_schema_lock,
};

const REPO_ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn main() {
    let action = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "check".to_owned());
    let root = Path::new(REPO_ROOT);
    let result = match action.as_str() {
        "check" => check_filesystem_schema_lock(root).and_then(|()| {
            check_filesystem_lock(root).map(|()| {
                println!("paired schema lock and executable asset lock are current");
            })
        }),
        "write" => write_schema_lock(root).and_then(|schema_path| {
            println!("wrote {}", schema_path.display());
            write_lock(root).map(|asset_path| {
                println!("wrote {}", asset_path.display());
            })
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
