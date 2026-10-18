//! Passes `MSA_CLIENT_ID` to the compiler. It comes from the environment or,
//! for local builds, from the git-ignored `.env` at the repository root. The
//! value is a public client id, never a secret, but it is not committed.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=MSA_CLIENT_ID");
    let env_file = Path::new("../.env");
    println!("cargo:rerun-if-changed={}", env_file.display());
    if std::env::var_os("MSA_CLIENT_ID").is_none()
        && let Some(client_id) = client_id_from(env_file)
    {
        println!("cargo:rustc-env=MSA_CLIENT_ID={client_id}");
    }
    tauri_build::build();
}

/// `MSA_CLIENT_ID=<value>` from a dotenv file, ignoring comments and quotes.
fn client_id_from(file: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(file).ok()?;
    contents.lines().find_map(|line| {
        let value = line.trim().strip_prefix("MSA_CLIENT_ID=")?;
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        let valid =
            !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        valid.then(|| value.to_owned())
    })
}
