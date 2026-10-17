//! The single place that maps the host OS and CPU to the names used by
//! Mojang rules, the Mojang Java runtime index and the Adoptium API.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsName {
    Windows,
    MacOs,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    X86,
    X86_64,
    Arm64,
}

/// The machine the game runs on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Platform {
    pub os: OsName,
    pub arch: Arch,
    /// OS version string matched by `os.version` rules (e.g. "10.0"); `None` when unknown.
    pub os_version: Option<String>,
}

impl Platform {
    pub fn new(os: OsName, arch: Arch) -> Self {
        Self {
            os,
            arch,
            os_version: None,
        }
    }

    /// The platform this binary was compiled for.
    pub fn current() -> Self {
        let os = if cfg!(target_os = "windows") {
            OsName::Windows
        } else if cfg!(target_os = "macos") {
            OsName::MacOs
        } else {
            OsName::Linux
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else if cfg!(target_arch = "x86") {
            Arch::X86
        } else {
            Arch::X86_64
        };
        Self::new(os, arch)
    }

    /// `os.name` in version JSON rules.
    pub fn mojang_os_name(&self) -> &'static str {
        match self.os {
            OsName::Windows => "windows",
            OsName::MacOs => "osx",
            OsName::Linux => "linux",
        }
    }

    /// `os.arch` in version JSON rules ("x86" means a 32-bit JVM).
    pub fn mojang_arch(&self) -> &'static str {
        match self.arch {
            Arch::X86 => "x86",
            Arch::X86_64 => "x86_64",
            Arch::Arm64 => "arm64",
        }
    }

    /// `${arch}` in legacy native classifiers (`natives-windows-${arch}`).
    pub fn bitness(&self) -> &'static str {
        match self.arch {
            Arch::X86 => "32",
            Arch::X86_64 | Arch::Arm64 => "64",
        }
    }

    /// Key in Mojang's Java runtime index; `None` where Mojang ships nothing (Linux arm64).
    pub fn mojang_runtime_platform(&self) -> Option<&'static str> {
        match (self.os, self.arch) {
            (OsName::Windows, Arch::X86_64) => Some("windows-x64"),
            (OsName::Windows, Arch::Arm64) => Some("windows-arm64"),
            (OsName::Windows, Arch::X86) => Some("windows-x86"),
            (OsName::MacOs, Arch::X86_64) => Some("mac-os"),
            (OsName::MacOs, Arch::Arm64) => Some("mac-os-arm64"),
            (OsName::Linux, Arch::X86_64) => Some("linux"),
            (OsName::Linux, Arch::X86) => Some("linux-i386"),
            (OsName::MacOs, Arch::X86) | (OsName::Linux, Arch::Arm64) => None,
        }
    }

    /// `os` and `architecture` query values of the Adoptium API.
    pub fn adoptium(&self) -> (&'static str, &'static str) {
        let os = match self.os {
            OsName::Windows => "windows",
            OsName::MacOs => "mac",
            OsName::Linux => "linux",
        };
        let arch = match self.arch {
            Arch::X86 => "x32",
            Arch::X86_64 => "x64",
            Arch::Arm64 => "aarch64",
        };
        (os, arch)
    }

    /// Separator between classpath entries.
    pub fn classpath_separator(&self) -> &'static str {
        if self.os == OsName::Windows { ";" } else { ":" }
    }

    /// Java executable inside a runtime's `bin/`. `javaw.exe` on Windows has no console window.
    pub fn java_executable(&self, console: bool) -> &'static str {
        match (self.os, console) {
            (OsName::Windows, false) => "javaw.exe",
            (OsName::Windows, true) => "java.exe",
            _ => "java",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_supported_platform_to_mojang_and_adoptium_names() {
        let cases = [
            (
                OsName::Windows,
                Arch::X86_64,
                "windows",
                "x86_64",
                Some("windows-x64"),
                ("windows", "x64"),
                ";",
            ),
            (
                OsName::Windows,
                Arch::Arm64,
                "windows",
                "arm64",
                Some("windows-arm64"),
                ("windows", "aarch64"),
                ";",
            ),
            (
                OsName::MacOs,
                Arch::X86_64,
                "osx",
                "x86_64",
                Some("mac-os"),
                ("mac", "x64"),
                ":",
            ),
            (
                OsName::MacOs,
                Arch::Arm64,
                "osx",
                "arm64",
                Some("mac-os-arm64"),
                ("mac", "aarch64"),
                ":",
            ),
            (
                OsName::Linux,
                Arch::X86_64,
                "linux",
                "x86_64",
                Some("linux"),
                ("linux", "x64"),
                ":",
            ),
            (
                OsName::Linux,
                Arch::Arm64,
                "linux",
                "arm64",
                None,
                ("linux", "aarch64"),
                ":",
            ),
        ];
        for (os, arch, rule_os, rule_arch, runtime, adoptium, separator) in cases {
            let platform = Platform::new(os, arch);
            assert_eq!(platform.mojang_os_name(), rule_os);
            assert_eq!(platform.mojang_arch(), rule_arch);
            assert_eq!(platform.mojang_runtime_platform(), runtime);
            assert_eq!(platform.adoptium(), adoptium);
            assert_eq!(platform.classpath_separator(), separator);
        }
    }

    #[test]
    fn uses_javaw_on_windows_without_a_console() {
        let windows = Platform::new(OsName::Windows, Arch::X86_64);
        assert_eq!(windows.java_executable(false), "javaw.exe");
        assert_eq!(windows.java_executable(true), "java.exe");
        assert_eq!(
            Platform::new(OsName::Linux, Arch::Arm64).java_executable(false),
            "java"
        );
    }

    #[test]
    fn legacy_native_bitness() {
        assert_eq!(Platform::new(OsName::Windows, Arch::X86).bitness(), "32");
        assert_eq!(Platform::new(OsName::Windows, Arch::X86_64).bitness(), "64");
    }
}
