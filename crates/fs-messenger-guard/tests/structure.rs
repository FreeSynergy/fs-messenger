//! What is true by structure, checked with a real compiler: nobody can build the capabilities, and the
//! core does not know this crate.

use std::path::Path;
use std::process::{Command, Output};

const CORE_TOML: &str = include_str!("../../fs-messenger-core/Cargo.toml");
const TRAITS: &str = include_str!("../../fs-messenger-core/src/provider/traits.rs");

fn check(dir: &Path, target: &Path, code: &str) -> Output {
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), code).unwrap();
    Command::new(env!("CARGO"))
        .args(["check", "-j", "4", "--offline"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap()
}

#[test]
fn the_core_has_no_dependency_on_this_crate() {
    assert!(!CORE_TOML.contains("fs-messenger-guard"));
}

#[test]
fn a_provider_gets_no_way_into_the_guard_or_the_store() {
    for name in [
        "Store",
        "MessengerRepo",
        "GuardPort",
        "EngineGuard",
        "SenderLookup",
        "Ingest",
    ] {
        assert!(!TRAITS.contains(name), "`Provider` mentions {name}");
    }
}

#[test]
fn nobody_outside_can_build_the_capabilities_or_reach_a_guard_from_a_gate() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let dir = tmp.join("structure-crate");
    std::fs::create_dir_all(&dir).unwrap();
    let guard = env!("CARGO_MANIFEST_DIR");
    let core = Path::new(guard).join("../fs-messenger-core");
    std::fs::write(
        dir.join("Cargo.toml"),
        format!(
            "[package]\nname = \"structure\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
             [dependencies]\nfs-messenger-guard = {{ path = \"{guard}\" }}\n\
             fs-messenger-core = {{ path = \"{}\" }}\n\n[workspace]\n",
            core.display()
        ),
    )
    .unwrap();
    std::fs::copy(
        Path::new(guard).join("../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .unwrap();
    let target = tmp.join("structure-target");

    let fine = check(
        &dir,
        &target,
        "pub fn f(g: &dyn fs_messenger_guard::Gate) -> &dyn fs_messenger_guard::Gate { g }\n",
    );
    assert!(
        fine.status.success(),
        "the harness itself is broken: {}",
        String::from_utf8_lossy(&fine.stderr)
    );
    for (code, why) in [
        (
            "pub fn f() { let _ = fs_messenger_core::GuardPort { repo: std::sync::Weak::new() }; }\n",
            "`GuardPort` must not be built from outside",
        ),
        (
            "pub fn f(e: std::sync::Arc<fs_messenger_guard::GuardEngine>) { let _ = fs_messenger_guard::LocalEnrolment::new(e); }\n",
            "`LocalEnrolment` must come only from `open`",
        ),
        (
            "pub async fn f(g: &dyn fs_messenger_guard::Gate) { let _ = g.state().await; }\n",
            "a `Gate` must have no `Guard` method",
        ),
        (
            "use fs_messenger_core::Messenger;\npub fn f(m: &dyn Messenger) { let _ = m.guard(); }\n",
            "`Messenger` must have no way to the guard",
        ),
        (
            "use fs_messenger_guard::Gate;\npub async fn f(g: &dyn Gate, c: &fs_messenger_guard::Caller, a: &fs_messenger_guard::Arguments) { let _ = g.check(c, \"x\", a, fs_messenger_core::ToolCallRisk::ReadsOnly).await; }\n",
            "`check` must take no risk from a caller",
        ),
    ] {
        let refused = check(&dir, &target, code);
        assert!(!refused.status.success(), "{why}");
        let errors = String::from_utf8_lossy(&refused.stderr);
        assert!(
            errors.contains("error[E"),
            "failed for another reason: {errors}"
        );
    }
}
