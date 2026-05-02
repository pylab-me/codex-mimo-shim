use std::env;

fn env_or_unknown(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| "unknown".to_string())
}

fn main() {
    println!(
        "cargo:rustc-env=BUILD_CARGO_LOCK_SHA256={}",
        env_or_unknown("BUILD_CARGO_LOCK_SHA256")
    );
    println!(
        "cargo:rustc-env=BUILD_CI_WORKFLOW_SHA256={}",
        env_or_unknown("BUILD_CI_WORKFLOW_SHA256")
    );
    println!(
        "cargo:rustc-env=BUILD_PUBLIC_BUILD_RS_SHA256={}",
        env_or_unknown("BUILD_PUBLIC_BUILD_RS_SHA256")
    );
    println!(
        "cargo:rustc-env=BUILD_SOURCE_REF={}",
        env_or_unknown("BUILD_SOURCE_REF")
    );
    println!(
        "cargo:rustc-env=BUILD_SOURCE_REPOSITORY={}",
        env_or_unknown("BUILD_SOURCE_REPOSITORY")
    );
    println!(
        "cargo:rustc-env=BUILD_RELEASE_VERSION={}",
        env_or_unknown("BUILD_RELEASE_VERSION")
    );
    println!(
        "cargo:rustc-env=BUILD_RELEASE_TARGET={}",
        env_or_unknown("BUILD_RELEASE_TARGET")
    );
}
