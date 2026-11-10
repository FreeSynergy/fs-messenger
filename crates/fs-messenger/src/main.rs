//! `fs-messenger`: the one program of FreeSynergy.Messenger (ADR 0063).
//!
//! Pattern: Command — the first word picks what runs. `app` or no word at all opens the window (feature `gui`); there is
//! no server and no command line yet (`DESIGN.md`, Not built). Without the feature `gui` the bare command prints the
//! help, so a machine without graphics does not hang on libraries.

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(dispatch(&words));
}

#[cfg(feature = "gui")]
fn dispatch(words: &[String]) -> i32 {
    match words.split_first() {
        Some((first, rest)) if first == "app" => fs_messenger_app::app_command(rest),
        _ => fs_messenger_app::app_command(words),
    }
}

#[cfg(not(feature = "gui"))]
fn dispatch(_words: &[String]) -> i32 {
    println!("fs-messenger: this build has no window (feature gui)");
    2
}
