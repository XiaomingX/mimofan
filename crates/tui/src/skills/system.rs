//! System-skill installer: bundles first-party skills and auto-installs them
//! on first launch.

use std::fs;
use std::path::Path;

const BUNDLED_SKILL_VERSION: &str = "6";
const SKILL_CREATOR_BODY: &str = include_str!("../../assets/skills/skill-creator/SKILL.md");
const DELEGATE_BODY: &str = include_str!("../../assets/skills/delegate/SKILL.md");
const V4_BEST_PRACTICES_BODY: &str = include_str!("../../assets/skills/v4-best-practices/SKILL.md");
const PLUGIN_CREATOR_BODY: &str = include_str!("../../assets/skills/plugin-creator/SKILL.md");
const SKILL_INSTALLER_BODY: &str = include_str!("../../assets/skills/skill-installer/SKILL.md");
const MCP_BUILDER_BODY: &str = include_str!("../../assets/skills/mcp-builder/SKILL.md");
const FLEET_MANAGER_BODY: &str = include_str!("../../assets/skills/fleet-manager/SKILL.md");
const DOCUMENTS_BODY: &str = include_str!("../../assets/skills/documents/SKILL.md");
const PRESENTATIONS_BODY: &str = include_str!("../../assets/skills/presentations/SKILL.md");
const SPREADSHEETS_BODY: &str = include_str!("../../assets/skills/spreadsheets/SKILL.md");
const PDF_BODY: &str = include_str!("../../assets/skills/pdf/SKILL.md");
const FEISHU_BODY: &str = include_str!("../../assets/skills/feishu/SKILL.md");
const SECURITY_AUDIT_BODY: &str = include_str!("../../assets/skills/security-audit/SKILL.md");
const VULN_HUNT_BODY: &str = include_str!("../../assets/skills/vuln-hunt/SKILL.md");

struct BundledSkill {
    name: &'static str,
    body: &'static str,
    introduced_in: u32,
}

const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        name: "skill-creator",
        body: SKILL_CREATOR_BODY,
        introduced_in: 1,
    },
    BundledSkill {
        name: "delegate",
        body: DELEGATE_BODY,
        introduced_in: 2,
    },
    BundledSkill {
        name: "v4-best-practices",
        body: V4_BEST_PRACTICES_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "plugin-creator",
        body: PLUGIN_CREATOR_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "skill-installer",
        body: SKILL_INSTALLER_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "mcp-builder",
        body: MCP_BUILDER_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "fleet-manager",
        body: FLEET_MANAGER_BODY,
        introduced_in: 4,
    },
    BundledSkill {
        name: "documents",
        body: DOCUMENTS_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "presentations",
        body: PRESENTATIONS_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "spreadsheets",
        body: SPREADSHEETS_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "pdf",
        body: PDF_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "feishu",
        body: FEISHU_BODY,
        introduced_in: 3,
    },
    BundledSkill {
        name: "security-audit",
        body: SECURITY_AUDIT_BODY,
        introduced_in: 4,
    },
    BundledSkill {
        name: "vuln-hunt",
        body: VULN_HUNT_BODY,
        introduced_in: 5,
    },
];

/// Whether a skill name matches one of the bundled first-party skills.
///
/// Used by `/skills` to distinguish user-created skills (which should be
/// surfaced prominently) from the always-installed bundle (which can be
/// rendered compactly when many skills are present).
#[must_use]
pub fn is_bundled_skill_name(name: &str) -> bool {
    BUNDLED_SKILLS.iter().any(|s| s.name == name)
}

/// Attempt to install a single bundled skill into `skills_dir`.
///
/// Returns `true` if installation occurred (fresh install or version bump).
fn install_one(
    skills_dir: &Path,
    skill: &BundledSkill,
    installed_version: Option<&str>,
) -> std::io::Result<bool> {
    let target_dir = skills_dir.join(skill.name);
    let target_file = target_dir.join("SKILL.md");
    let dir_exists = target_dir.exists();
    let installed_number = installed_version.and_then(|value| value.parse::<u32>().ok());

    let should_install = match (installed_version, installed_number, dir_exists) {
        // Fresh install: neither marker nor directory.
        (None, _, false) => true,
        // Newly bundled skill: add it for older system-skill installs.
        (Some(_), Some(version), _) if version < skill.introduced_in => true,
        // Version bump for an existing skill: refresh only if the user has not
        // intentionally deleted that skill directory.
        (Some(version), _, true) if version != BUNDLED_SKILL_VERSION => true,
        // Every other case: current install, user-deleted dir, or pre-existing
        // user-owned skill without our marker.
        _ => false,
    };

    if should_install {
        fs::create_dir_all(&target_dir)?;
        fs::write(&target_file, skill.body)?;
    }
    Ok(should_install)
}

/// Install bundled system skills into `skills_dir`.
///
/// Behaviour:
/// - Fresh install (no marker, no dir): installs every bundled skill, then
///   writes the version marker.
/// - Version bump (marker present with older version): re-installs any existing
///   bundled skill and installs newly introduced bundled skills.
/// - User deleted a skill dir while marker still present at same version: leaves
///   it gone.
/// - Idempotent: calling twice with no changes is a no-op.
///
/// Errors are I/O errors from the filesystem; the caller should log them but not
/// abort startup.
pub fn install_system_skills(skills_dir: &Path) -> std::io::Result<()> {
    let marker = skills_dir.join(".system-installed-version");

    let installed_version = fs::read_to_string(&marker)
        .ok()
        .map(|s| s.trim().to_string());

    let mut changed = false;
    for skill in BUNDLED_SKILLS {
        changed |= install_one(skills_dir, skill, installed_version.as_deref())?;
    }

    if changed {
        fs::create_dir_all(skills_dir)?;
        fs::write(&marker, BUNDLED_SKILL_VERSION)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vuln_hunt_is_bundled_skill() {
        // plan `plans/12` Phase 4：vuln-hunt 升级为编译期内嵌 bundled skill。
        assert!(
            is_bundled_skill_name("vuln-hunt"),
            "vuln-hunt must be a bundled skill"
        );
        assert!(BUNDLED_SKILLS.iter().any(|s| s.name == "vuln-hunt"));
    }

    #[test]
    fn bundled_skills_are_non_empty() {
        for s in BUNDLED_SKILLS {
            assert!(
                !s.body.trim().is_empty(),
                "bundled skill '{}' has an empty body",
                s.name
            );
        }
    }
}
