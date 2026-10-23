//! Passes `MSA_CLIENT_ID` and `CURSEFORGE_API_KEY` to the compiler. They come
//! from the environment (CI) or, for local builds, from the git-ignored
//! `.env` at the repository root, and are never committed. The client id is
//! public; the CurseForge key is a third-party launcher key, compiled in the
//! way every launcher ships it.

use std::path::Path;

/// Which characters a setting's value may hold.
type Allowed = fn(char) -> bool;

/// Build-time settings read from `.env` when the environment lacks them,
/// with the characters each value may hold.
const FROM_ENV_FILE: [(&str, Allowed); 2] = [
    ("MSA_CLIENT_ID", |c| c.is_ascii_alphanumeric() || c == '-'),
    ("CURSEFORGE_API_KEY", |c| {
        c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\')
    }),
];

fn main() {
    println!("cargo:rerun-if-env-changed=CINEPOL_UPDATER_PUBKEY");
    let env_file = Path::new("../.env");
    println!("cargo:rerun-if-changed={}", env_file.display());
    for (name, allowed) in FROM_ENV_FILE {
        println!("cargo:rerun-if-env-changed={name}");
        if std::env::var_os(name).is_none()
            && let Some(value) = value_from(env_file, name, allowed)
        {
            println!("cargo:rustc-env={name}={value}");
        }
    }
    tauri_build::build();
}

/// `<name>=<value>` from a dotenv file, ignoring comments and quotes; `None`
/// when the value is empty or holds a character `allowed` refuses.
fn value_from(file: &Path, name: &str, allowed: Allowed) -> Option<String> {
    let contents = std::fs::read_to_string(file).ok()?;
    let prefix = format!("{name}=");
    contents.lines().find_map(|line| {
        let value = line.trim().strip_prefix(prefix.as_str())?;
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        let valid = !value.is_empty() && value.chars().all(allowed);
        valid.then(|| value.to_owned())
    })
}
