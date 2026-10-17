//! Mojang `rules` evaluation for libraries and arguments.

use std::collections::{BTreeMap, BTreeSet};

use super::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    Allow,
    Disallow,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OsCondition {
    pub name: Option<String>,
    pub arch: Option<String>,
    /// A regular expression matched against the OS version.
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub action: RuleAction,
    pub os: Option<OsCondition>,
    /// Feature name -> required value (e.g. `has_custom_resolution: true`).
    pub features: BTreeMap<String, bool>,
}

impl Rule {
    pub fn allow() -> Self {
        Self {
            action: RuleAction::Allow,
            os: None,
            features: BTreeMap::new(),
        }
    }
}

/// Launch features that are switched on (`is_demo_user`, `has_custom_resolution`,
/// `has_quick_plays_support`, `is_quick_play_multiplayer`, ...).
pub type Features = BTreeSet<String>;

fn os_matches(condition: &OsCondition, platform: &Platform) -> bool {
    let name_ok = condition
        .name
        .as_deref()
        .is_none_or(|name| name == platform.mojang_os_name());
    let arch_ok = condition
        .arch
        .as_deref()
        .is_none_or(|arch| arch == platform.mojang_arch());
    let version_ok = condition.version.as_deref().is_none_or(|pattern| {
        let Some(version) = platform.os_version.as_deref() else {
            return false;
        };
        regex_lite::Regex::new(pattern).is_ok_and(|regex| regex.is_match(version))
    });
    name_ok && arch_ok && version_ok
}

fn rule_matches(rule: &Rule, platform: &Platform, features: &Features) -> bool {
    let os_ok = rule.os.as_ref().is_none_or(|os| os_matches(os, platform));
    let features_ok = rule
        .features
        .iter()
        .all(|(name, wanted)| features.contains(name) == *wanted);
    os_ok && features_ok
}

/// Mojang semantics: no rules allows; otherwise start disallowed and let the
/// last matching rule decide.
pub fn rules_allow(rules: &[Rule], platform: &Platform, features: &Features) -> bool {
    if rules.is_empty() {
        return true;
    }
    rules.iter().fold(false, |allowed, rule| {
        if rule_matches(rule, platform, features) {
            rule.action == RuleAction::Allow
        } else {
            allowed
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::launch::{Arch, OsName};

    fn os_rule(
        action: RuleAction,
        name: Option<&str>,
        arch: Option<&str>,
        version: Option<&str>,
    ) -> Rule {
        Rule {
            action,
            os: Some(OsCondition {
                name: name.map(str::to_owned),
                arch: arch.map(str::to_owned),
                version: version.map(str::to_owned),
            }),
            features: BTreeMap::new(),
        }
    }

    fn feature_rule(name: &str) -> Rule {
        Rule {
            action: RuleAction::Allow,
            os: None,
            features: BTreeMap::from([(name.to_owned(), true)]),
        }
    }

    const ALL: [(OsName, Arch); 6] = [
        (OsName::Windows, Arch::X86_64),
        (OsName::Windows, Arch::Arm64),
        (OsName::MacOs, Arch::X86_64),
        (OsName::MacOs, Arch::Arm64),
        (OsName::Linux, Arch::X86_64),
        (OsName::Linux, Arch::Arm64),
    ];

    #[test]
    fn no_rules_allow_everywhere() {
        for (os, arch) in ALL {
            assert!(rules_allow(&[], &Platform::new(os, arch), &Features::new()));
        }
    }

    #[test]
    fn os_name_rules_select_one_os_across_architectures() {
        let osx_only = [os_rule(RuleAction::Allow, Some("osx"), None, None)];
        for (os, arch) in ALL {
            assert_eq!(
                rules_allow(&osx_only, &Platform::new(os, arch), &Features::new()),
                os == OsName::MacOs
            );
        }
    }

    #[test]
    fn allow_then_disallow_excludes_one_os() {
        // lwjgl 2.9.4 in 1.12.2: everywhere except macOS.
        let rules = [
            Rule::allow(),
            os_rule(RuleAction::Disallow, Some("osx"), None, None),
        ];
        for (os, arch) in ALL {
            assert_eq!(
                rules_allow(&rules, &Platform::new(os, arch), &Features::new()),
                os != OsName::MacOs
            );
        }
    }

    #[test]
    fn x86_arch_rules_only_match_32_bit() {
        let rules = [os_rule(RuleAction::Allow, None, Some("x86"), None)];
        assert!(rules_allow(
            &rules,
            &Platform::new(OsName::Windows, Arch::X86),
            &Features::new()
        ));
        for (os, arch) in ALL {
            assert!(!rules_allow(
                &rules,
                &Platform::new(os, arch),
                &Features::new()
            ));
        }
    }

    #[test]
    fn os_version_rules_use_a_regex_and_fail_closed_when_unknown() {
        let rules = [os_rule(
            RuleAction::Allow,
            Some("windows"),
            None,
            Some("^10\\."),
        )];
        let mut win10 = Platform::new(OsName::Windows, Arch::X86_64);
        assert!(!rules_allow(&rules, &win10, &Features::new()));
        win10.os_version = Some("10.0".into());
        assert!(rules_allow(&rules, &win10, &Features::new()));
        win10.os_version = Some("6.1".into());
        assert!(!rules_allow(&rules, &win10, &Features::new()));
    }

    #[test]
    fn feature_rules_need_the_feature_enabled() {
        let rules = [feature_rule("has_custom_resolution")];
        let platform = Platform::new(OsName::Linux, Arch::X86_64);
        assert!(!rules_allow(&rules, &platform, &Features::new()));
        assert!(rules_allow(
            &rules,
            &platform,
            &Features::from(["has_custom_resolution".to_owned()])
        ));
    }
}
