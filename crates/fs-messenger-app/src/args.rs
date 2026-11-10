//! The words after the program name.
//!
//! Pattern: none (a small pure parser).

use std::path::PathBuf;

/// The help text.
pub const HELP: &str = "fs-messenger [app] [--store FILE]\n\
\n\
Opens the window of FreeSynergy.Messenger.\n\
  --store FILE   the encrypted store file (default: the data folder of the user)\n\
  --version      print the version\n\
  --help         print this text";

/// What the words ask for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// `--help`.
    pub help: bool,
    /// `--version`.
    pub version: bool,
    /// `--store FILE`.
    pub store: Option<PathBuf>,
}

/// Read the words (without the program name and without a leading `app`).
///
/// # Errors
/// The message for the person: an unknown word, or `--store` without a file.
pub fn parse(words: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = words.iter();
    while let Some(word) = it.next() {
        match word.as_str() {
            "--help" | "-h" => args.help = true,
            "--version" | "-V" => args.version = true,
            "--store" => {
                let file = it.next().ok_or("--store needs a file")?;
                args.store = Some(PathBuf::from(file));
            }
            other => return Err(format!("unknown word: {other}")),
        }
    }
    Ok(args)
}
