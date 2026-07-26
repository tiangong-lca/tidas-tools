use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tidas_dist::{PackageRequest, package, render_package_metadata, verify};

#[derive(Debug, Parser)]
#[command(
    name = "tidas-dist",
    about = "Internal deterministic distribution builder for the native tidas CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Print the workspace release version.
    Version,
    /// Build one deterministic platform archive and checksum.
    Package {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        license: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Verify checksum, archive contract, and optionally run packaged smoke probes.
    Verify {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        checksum: PathBuf,
        #[arg(long)]
        target: String,
        #[arg(long)]
        smoke: bool,
    },
    /// Generate Homebrew and Winget metadata from the five exact archive checksums.
    Metadata {
        #[arg(long)]
        release_base_url: String,
        #[arg(long)]
        artifacts_dir: PathBuf,
        #[arg(long)]
        output_dir: PathBuf,
    },
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), tidas_dist::DistError> {
    let version = env!("CARGO_PKG_VERSION");
    match Cli::parse().command {
        Commands::Version => println!("{version}"),
        Commands::Package {
            binary,
            license,
            target,
            output_dir,
        } => {
            let artifact = package(&PackageRequest {
                binary: &binary,
                license: &license,
                target: &target,
                version,
                output_dir: &output_dir,
            })?;
            println!("{}", serde_json::to_string(&artifact)?);
        }
        Commands::Verify {
            archive,
            checksum,
            target,
            smoke,
        } => {
            let manifest = verify(&archive, &checksum, &target, version, smoke)?;
            println!("{}", serde_json::to_string(&manifest)?);
        }
        Commands::Metadata {
            release_base_url,
            artifacts_dir,
            output_dir,
        } => {
            for path in
                render_package_metadata(version, &release_base_url, &artifacts_dir, &output_dir)?
            {
                println!("{}", path.display());
            }
        }
    }
    Ok(())
}
