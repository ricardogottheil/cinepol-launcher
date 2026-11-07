//! What a crash leaves behind, and how it is cleaned before it is shared.

/// The JVM writes `hs_err_pid<pid>.log` into its working folder (the game
/// folder) when it crashes itself.
pub const JVM_CRASH_LOG_PREFIX: &str = "hs_err_pid";

const MASK: &str = "<redacted>";
const ENVIRONMENT_OMITTED: &str = "<environment variables omitted>";
/// Microsoft and Minecraft tokens are JWTs; shorter `eyJ` text is not one.
const MIN_JWT_LEN: usize = 20;

/// A plain file name: no separators, drive letters, NUL or dot segments.
pub fn is_plain_file_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':', '\0']) && name != "." && name != ".."
}

/// A crash report the game writes into `crash-reports/`: a plain `.txt` name.
pub fn is_crash_report_name(name: &str) -> bool {
    is_plain_file_name(name) && has_extension(name, "txt")
}

/// `hs_err_pid<pid>.log`, as a plain file name.
pub fn is_jvm_crash_log_name(name: &str) -> bool {
    is_plain_file_name(name) && name.starts_with(JVM_CRASH_LOG_PREFIX) && has_extension(name, "log")
}

fn has_extension(name: &str, extension: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(stem, found)| !stem.is_empty() && found.eq_ignore_ascii_case(extension))
}

/// Removes what must never leave the player's computer: the session token
/// (`--accessToken <token>` in the JVM's command line, and any JWT) and the
/// environment variables a JVM crash log lists (they may hold API keys).
pub fn redact_crash_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_environment = false;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        if in_environment {
            if content.trim().is_empty() {
                in_environment = false;
                out.push_str(line);
            }
            continue;
        }
        if content.trim_start().starts_with("Environment Variables:") {
            in_environment = true;
            out.push_str(content);
            out.push('\n');
            out.push_str(ENVIRONMENT_OMITTED);
            out.push('\n');
            continue;
        }
        out.push_str(&mask_jwts(&mask_access_token(line)));
    }
    out
}

/// `--accessToken <value>` keeps the flag and loses the value.
fn mask_access_token(line: &str) -> String {
    const FLAG: &str = "--accessToken";
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find(FLAG) {
        let after_flag = at + FLAG.len();
        out.push_str(&rest[..after_flag]);
        let tail = &rest[after_flag..];
        let spaces = tail.len() - tail.trim_start_matches([' ', '\t']).len();
        let value = tail[spaces..]
            .find(char::is_whitespace)
            .unwrap_or(tail.len() - spaces);
        if value == 0 {
            rest = tail;
            continue;
        }
        out.push_str(&tail[..spaces]);
        out.push_str(MASK);
        rest = &tail[spaces + value..];
    }
    out.push_str(rest);
    out
}

/// `eyJ` followed by base64url segments with at least two dots.
fn mask_jwts(line: &str) -> String {
    let is_token_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("eyJ") {
        out.push_str(&rest[..at]);
        let candidate = &rest[at..];
        let len = candidate
            .find(|c| !is_token_char(c))
            .unwrap_or(candidate.len());
        let token = &candidate[..len];
        if token.len() >= MIN_JWT_LEN && token.matches('.').count() >= 2 {
            out.push_str(MASK);
        } else {
            out.push_str(token);
        }
        rest = &candidate[len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_file_names_are_plain_and_of_the_right_kind() {
        assert!(is_crash_report_name("crash-2026-10-19_12.00.00-client.txt"));
        assert!(is_jvm_crash_log_name("hs_err_pid4242.log"));
        for bad in [
            "",
            ".",
            "..",
            "../x.txt",
            "a/b.txt",
            "a\\b.txt",
            "C:x.txt",
            "a\0b",
            ".txt",
            "crash.log",
        ] {
            assert!(!is_crash_report_name(bad), "{bad:?}");
        }
        for bad in [
            "hs_err_pid1.txt",
            "options.log",
            "../hs_err_pid1.log",
            "hs_err_pid1",
            "latest.log",
        ] {
            assert!(!is_jvm_crash_log_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn masks_the_access_token_in_a_command_line() {
        assert_eq!(
            redact_crash_text(
                "java_command: net.minecraft.client.main.Main --username Polispol1 --accessToken abc.def-123 --version 1.21.1\n"
            ),
            "java_command: net.minecraft.client.main.Main --username Polispol1 --accessToken <redacted> --version 1.21.1\n"
        );
        assert_eq!(redact_crash_text("--accessToken"), "--accessToken");
    }

    #[test]
    fn masks_jwts_anywhere() {
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJl";
        assert_eq!(
            redact_crash_text(&format!("token={jwt}, user=Polispol1")),
            "token=<redacted>, user=Polispol1"
        );
        assert_eq!(redact_crash_text("eyJ short"), "eyJ short");
    }

    #[test]
    fn omits_the_environment_variables_of_a_jvm_crash_log() {
        let log = "OS: Windows 11\r\n\
                   Environment Variables:\r\n\
                   JAVA_HOME=C:\\Java\r\n\
                   CURSEFORGE_API_KEY=secret\r\n\
                   \r\n\
                   ---------------  S Y S T E M  ---------------\r\n";
        assert_eq!(
            redact_crash_text(log),
            "OS: Windows 11\r\n\
             Environment Variables:\n\
             <environment variables omitted>\n\
             \r\n\
             ---------------  S Y S T E M  ---------------\r\n"
        );
    }
}
