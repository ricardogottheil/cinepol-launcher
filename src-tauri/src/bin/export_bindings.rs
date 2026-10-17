//! Writes `src/ipc/bindings.ts` from the Rust DTOs. Run with `pnpm bindings`.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/ipc/bindings.ts");
    match std::fs::write(&target, cinepol_launcher_lib::bindings::render()) {
        Ok(()) => {
            println!("Wrote {}", target.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("Failed to write {}: {error}", target.display());
            ExitCode::FAILURE
        }
    }
}
