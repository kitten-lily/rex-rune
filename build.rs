use std::path::Path;
use std::process::Command;

#[allow(dead_code)]
#[path = "src/dracula_pro/derive.rs"]
mod derive;
#[path = "build/dracula_pro.rs"]
mod dracula_pro;

fn output(command: &mut Command) -> Option<String> {
    let result = command.output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    // Watch the resolved paths too: .git is a file in a linked worktree.
    for name in ["HEAD", "refs/heads", "packed-refs"] {
        if let Some(path) = output(Command::new("git").args(["rev-parse", "--git-path", name])) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let root = output(Command::new("git").args(["rev-parse", "--show-toplevel"]));
    let commit = root
        .filter(|root| Some(root) == std::env::var("CARGO_MANIFEST_DIR").ok().as_ref())
        .and_then(|_| output(Command::new("git").args(["rev-parse", "--short=7", "HEAD"])));
    // Release builders can supply the same reproducible epoch used by package-release.py.
    let date = commit.as_ref().and_then(|_| {
        let mut command = Command::new("date");
        command.env("TZ", "America/Los_Angeles");
        if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH") {
            command.args(["--date", &format!("@{epoch}")]);
        }
        output(command.arg("+%Y-%m-%d %Z"))
    });
    println!(
        "cargo:rustc-env=REX_BUILD_COMMIT={}",
        commit.as_deref().unwrap_or("unknown")
    );
    println!(
        "cargo:rustc-env=REX_BUILD_DATE={}",
        date.as_deref().unwrap_or("unknown")
    );
    dracula_pro();
}

/// Generates `$OUT_DIR/dracula_pro.rs` from the local Dracula Pro checkout, if there is one.
fn dracula_pro() {
    for name in [
        "REX_DRACULA_PRO",
        "REX_DRACULA_PRO_DIR",
        "XDG_DATA_HOME",
        "HOME",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    println!("cargo:rerun-if-changed=build");
    println!("cargo::rustc-check-cfg=cfg(rex_dracula_pro)");
    println!("cargo::rustc-check-cfg=cfg(rex_dracula_pro_chrome)");
    let dir = dracula_pro::source_dir(|name| std::env::var(name).ok());
    let out = dracula_pro::generate(dir.as_deref());
    for path in &out.watch {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    for warning in &out.warnings {
        println!("cargo:warning=Dracula Pro: {warning}");
    }
    let target = Path::new(&std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("dracula_pro.rs");
    if std::fs::read_to_string(&target).ok().as_deref() != Some(out.code.as_str()) {
        std::fs::write(&target, &out.code).expect("write dracula_pro.rs");
    }
    if out.themes > 0 {
        println!("cargo:rustc-cfg=rex_dracula_pro");
    }
    if out.chromes > 0 {
        println!("cargo:rustc-cfg=rex_dracula_pro_chrome");
    }
}
