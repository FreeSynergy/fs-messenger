//! gpg in a throwaway home, for the interoperability tests.

use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

/// gpg in a home of its own. Arguments are one string, split at spaces.
pub struct Gpg(tempfile::TempDir);

impl Gpg {
    pub fn new() -> Option<Self> {
        if Command::new("gpg").arg("--version").output().is_err() {
            println!("SKIPPED: no gpg on this machine");
            return None;
        }
        let dir = tempfile::Builder::new().prefix("g").tempdir().unwrap();
        let mode = PermissionsExt::from_mode(0o700);
        std::fs::set_permissions(dir.path(), mode).unwrap();
        Some(Self(dir))
    }

    /// Runs gpg and prints what it said, verbatim; answers exit status and output.
    pub fn run(&self, args: &str, input: &[u8]) -> (bool, Vec<u8>) {
        let mut cmd = Command::new("gpg");
        cmd.env("GNUPGHOME", self.0.path());
        cmd.arg("--homedir").arg(self.0.path());
        cmd.args("--batch --no-tty --pinentry-mode loopback --no-auto-key-locate".split(' '));
        cmd.args(["--passphrase", "", "--trust-model", "always"]);
        cmd.args(args.split(' '));
        cmd.env("LC_ALL", "C").env("LANGUAGE", "C");
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let out = child.wait_with_output().unwrap();
        println!("--- gpg {args}: {}", out.status);
        print!("{}", String::from_utf8_lossy(&out.stderr));
        if !args.contains("--armor") && !args.contains("--export") {
            print!("{}", String::from_utf8_lossy(&out.stdout));
        }
        (out.status.success(), out.stdout)
    }

    /// Ed25519 and Cv25519, no passphrase; `version` is empty, or the lines naming a key version.
    pub fn generate(&self, address: &str, version: &str) {
        let batch = format!(
            "%no-protection\nKey-Type: eddsa\nKey-Curve: ed25519\nKey-Usage: cert,sign\nSubkey-Type: ecdh\n\
             Subkey-Curve: cv25519\nSubkey-Usage: encr\n{version}Name-Email: {address}\nExpire-Date: 0\n%commit\n"
        );
        assert!(self.run("--gen-key", batch.as_bytes()).0);
    }

    pub fn opens(&self, armored: &[u8]) -> bool {
        self.run("--list-packets", armored);
        self.run("--decrypt", armored).0
    }
}

impl Drop for Gpg {
    fn drop(&mut self) {
        for args in ["--kill all", "--remove-socketdir"] {
            let mut cmd = Command::new("gpgconf");
            cmd.env("GNUPGHOME", self.0.path()).args(args.split(' '));
            let _ = cmd.output();
        }
    }
}
