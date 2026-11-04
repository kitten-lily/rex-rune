//! Picks the chrome palette at build time: `REX_CHROME` is `midnight` (the default),
//! `rose-pine` or `rose-pine-moon`, compiled in as `cfg(chrome = "…")`.
const CHROMES: [&str; 3] = ["midnight", "rose-pine", "rose-pine-moon"];

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=REX_CHROME");
    let values = CHROMES.map(|c| format!("\"{c}\"")).join(", ");
    println!("cargo::rustc-check-cfg=cfg(chrome, values({values}))");
    let chrome = std::env::var("REX_CHROME").unwrap_or_default();
    let chrome = if chrome.is_empty() {
        "midnight"
    } else {
        &chrome
    };
    if !CHROMES.contains(&chrome) {
        panic!(
            "REX_CHROME={chrome} is not a chrome; use one of {}",
            CHROMES.join(", ")
        );
    }
    println!("cargo::rustc-cfg=chrome=\"{chrome}\"");
}
