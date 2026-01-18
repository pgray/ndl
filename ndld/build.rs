use std::process::Command;

fn main() {
    // Get git describe output (tag, commits since tag, hash, dirty status)
    let git_describe = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo::rerun-if-changed=.git/HEAD");
    println!("cargo::rerun-if-changed=.git/refs/tags");
    println!("cargo::rustc-env=NDLD_GIT_VERSION={}", git_describe);
}
