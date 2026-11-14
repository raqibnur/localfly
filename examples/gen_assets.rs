//! Generates shell completions and man pages into packaging/.
//! Run: `cargo run --example gen_assets` (also done by CI and releases).

use clap::{Command, CommandFactory};
use clap_complete::{Shell, generate_to};
use std::path::PathBuf;

fn out_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("packaging").join("generated")
}

fn main() {
    let dir = out_dir();
    let comp_dir = dir.join("completions");
    let man_dir = dir.join("man");
    std::fs::create_dir_all(&comp_dir).unwrap();
    std::fs::create_dir_all(&man_dir).unwrap();

    for (bin, mut cmd) in bins() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            generate_to(shell, &mut cmd, bin, &comp_dir).unwrap();
        }
        let man = clap_mangen::Man::new(cmd);
        let mut buf = Vec::new();
        man.render(&mut buf).unwrap();
        std::fs::write(man_dir.join(format!("{bin}.1")), buf).unwrap();
    }
    println!("wrote {}", dir.display());
}

fn bins() -> Vec<(&'static str, Command)> {
    vec![
        ("localfly", localfly_lib::cli::Cli::command()),
        ("localflyd", localfly_lib::cli::DaemonArgs::command()),
    ]
}
