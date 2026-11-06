//! C3: the real check: a crate that does not ask for `test-support` cannot name `open`.
//!
//! Builds a small crate against this one, in its own target folder, so the first run takes a while.
//! The crate that may name `open_turso` must compile, or a failure would prove nothing.

use std::path::Path;
use std::process::{Command, Output};

fn check(dir: &Path, target: &Path, call: &str) -> Output {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        src.join("lib.rs"),
        format!("pub fn f() {{ let _ = fs_messenger_core::MessengerRepo::{call}; }}\n"),
    )
    .unwrap();
    Command::new(env!("CARGO"))
        .args(["check", "-j", "4", "--offline"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap()
}

#[test]
fn c3_open_does_not_exist_without_test_support() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let dir = tmp.join("compile-fail-crate");
    std::fs::create_dir_all(&dir).unwrap();
    let core = env!("CARGO_MANIFEST_DIR");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"compile-fail\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
             [dependencies]\nfs-messenger-core = {{ path = \"{core}\" }}\n\n[workspace]\n"
        ),
    )
    .unwrap();
    // The same versions as the crate itself, and nothing new to fetch.
    std::fs::copy(
        Path::new(core).join("../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .unwrap();
    let target = tmp.join("compile-fail-target");

    let allowed = check(&dir, &target, "open_turso");
    assert!(
        allowed.status.success(),
        "the harness itself is broken: {}",
        String::from_utf8_lossy(&allowed.stderr)
    );

    let refused = check(&dir, &target, "open");
    let errors = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "`open` must not exist here");
    assert!(
        errors.contains("E0599"),
        "failed for another reason: {errors}"
    );
}
