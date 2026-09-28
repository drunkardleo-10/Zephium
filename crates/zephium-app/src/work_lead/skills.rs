//! Skills: a folder with `SKILL.md` whose frontmatter names the skill, says
//! in one line when it applies, and optionally lists tools and a model role.
//! Only names and descriptions sit in the prompt; `load_skill` returns the
//! body. Built-ins ship in the app; a person's skills live in
//! `<data dir>/skills/<profile>/<name>/SKILL.md` and override a built-in of
//! the same name.
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use zephium_core::ids::ProfileId;
use zephium_core::work::model::WorkModelRole;

pub const MAX_SKILL_NAME: usize = 40;
pub const MAX_SKILL_DESCRIPTION: usize = 200;
pub const MAX_SKILL_BYTES: usize = 16 * 1024;
pub const MAX_USER_SKILLS: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Tools the skill expects; advisory, never a grant.
    pub tools: Vec<String>,
    pub role: Option<WorkModelRole>,
    pub body: String,
    pub builtin: bool,
}

/// Why a `SKILL.md` was not admitted, in closed words for Settings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillFault {
    NoFrontmatter,
    Name,
    Description,
    Role,
    TooLarge,
    Empty,
}

static ROOT: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The app's data directory; set once by the composition root.
pub fn install_root(data_dir: PathBuf) {
    if let Ok(mut root) = ROOT.write() {
        *root = Some(data_dir);
    }
}

/// Where this profile's own skills live, once the root is installed.
pub fn user_dir(profile: ProfileId) -> Option<PathBuf> {
    let root = ROOT.read().ok()?.clone()?;
    Some(root.join("skills").join(profile.to_string()))
}

const BUILTIN: &[&str] = &[
    include_str!("../../skills/trip-planning/SKILL.md"),
    include_str!("../../skills/system-design/SKILL.md"),
    include_str!("../../skills/compare-and-choose/SKILL.md"),
    include_str!("../../skills/research/SKILL.md"),
    include_str!("../../skills/explain-a-subject/SKILL.md"),
    include_str!("../../skills/today-from-my-messages/SKILL.md"),
    include_str!("../../skills/fix-a-bug/SKILL.md"),
    include_str!("../../skills/job-search/SKILL.md"),
    include_str!("../../skills/learning-path/SKILL.md"),
];

pub fn builtins() -> Vec<Skill> {
    BUILTIN
        .iter()
        .filter_map(|text| parse(text).ok())
        .map(|skill| Skill {
            builtin: true,
            ..skill
        })
        .collect()
}

/// Built-ins, then the profile's own skills replacing any of the same name.
pub fn load(profile: ProfileId) -> Vec<Skill> {
    let mut skills = builtins();
    if let Some(dir) = user_dir(profile) {
        for skill in read_dir(&dir) {
            match skills.iter_mut().find(|known| known.name == skill.name) {
                Some(known) => *known = skill,
                None => skills.push(skill),
            }
        }
    }
    skills
}

/// Admitted skills in a folder of skill folders, by name order.
pub fn read_dir(dir: &Path) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills: Vec<Skill> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .take(MAX_USER_SKILLS * 2)
        .filter_map(|entry| {
            let path = entry.path().join("SKILL.md");
            let meta = std::fs::symlink_metadata(&path).ok()?;
            if !meta.file_type().is_file() || meta.len() > MAX_SKILL_BYTES as u64 {
                return None;
            }
            parse(&std::fs::read_to_string(path).ok()?).ok()
        })
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills.dedup_by(|a, b| a.name == b.name);
    skills.truncate(MAX_USER_SKILLS);
    skills
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_SKILL_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// Parses a `SKILL.md`: `---`, `key: value` lines, `---`, then the body.
pub fn parse(text: &str) -> Result<Skill, SkillFault> {
    if text.len() > MAX_SKILL_BYTES {
        return Err(SkillFault::TooLarge);
    }
    let text = text.trim_start_matches('\u{feff}');
    let rest = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
        .ok_or(SkillFault::NoFrontmatter)?;
    let end = rest.find("\n---").ok_or(SkillFault::NoFrontmatter)?;
    let (head, body) = (&rest[..end], &rest[end + 4..]);
    let body = body
        .strip_prefix('\n')
        .or_else(|| body.strip_prefix("\r\n"))
        .unwrap_or(body)
        .trim()
        .to_owned();
    let (mut name, mut description, mut tools, mut role) = (None, None, Vec::new(), None);
    for line in head.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim();
        match key.trim() {
            "name" => name = Some(value.to_owned()),
            "description" => description = Some(value.to_owned()),
            "tools" => {
                tools = value
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|tool| tool.trim().trim_matches('"').to_owned())
                    .filter(|tool| !tool.is_empty())
                    .take(24)
                    .collect()
            }
            "role" => {
                role = Some(match value {
                    "lead" => WorkModelRole::Lead,
                    "page" => WorkModelRole::Page,
                    "light" => WorkModelRole::Light,
                    _ => return Err(SkillFault::Role),
                })
            }
            _ => {}
        }
    }
    let name = name.filter(|n| valid_name(n)).ok_or(SkillFault::Name)?;
    let description = description
        .filter(|d| {
            !d.is_empty()
                && d.chars().count() <= MAX_SKILL_DESCRIPTION
                && !d.chars().any(char::is_control)
        })
        .ok_or(SkillFault::Description)?;
    if body.is_empty() {
        return Err(SkillFault::Empty);
    }
    Ok(Skill {
        name,
        description,
        tools,
        role,
        body,
        builtin: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_is_frontmatter_and_a_body() {
        let skill = parse(
            "---\nname: weekly-review\ndescription: Review my week from calendar and notes.\ntools: [web_search, start_part]\nrole: light\n---\n\n# Weekly review\nDo the thing.\n",
        )
        .unwrap();
        assert_eq!(skill.name, "weekly-review");
        assert_eq!(skill.tools, ["web_search", "start_part"]);
        assert_eq!(skill.role, Some(WorkModelRole::Light));
        assert!(skill.body.starts_with("# Weekly review"));
        for (text, fault) in [
            ("name: x\n", SkillFault::NoFrontmatter),
            (
                "---\nname: Bad Name\ndescription: d\n---\nbody",
                SkillFault::Name,
            ),
            ("---\nname: ok\n---\nbody", SkillFault::Description),
            (
                "---\nname: ok\ndescription: d\nrole: boss\n---\nbody",
                SkillFault::Role,
            ),
            (
                "---\nname: ok\ndescription: d\n---\n  \n",
                SkillFault::Empty,
            ),
        ] {
            assert_eq!(parse(text), Err(fault), "{text}");
        }
    }

    #[test]
    fn every_builtin_parses_and_fits() {
        let builtins = builtins();
        assert_eq!(builtins.len(), BUILTIN.len());
        for skill in &builtins {
            assert!(skill.body.len() < 6_000, "{} is long", skill.name);
        }
    }

    #[test]
    fn a_persons_skill_overrides_a_builtin_of_the_same_name() {
        let dir = tempfile::tempdir().unwrap();
        let profile = ProfileId::from(7);
        install_root(dir.path().to_owned());
        let mine = user_dir(profile).unwrap().join("research");
        std::fs::create_dir_all(&mine).unwrap();
        std::fs::write(
            mine.join("SKILL.md"),
            "---\nname: research\ndescription: My own research.\n---\nMine.",
        )
        .unwrap();
        let other = user_dir(profile).unwrap().join("broken");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("SKILL.md"), "no frontmatter").unwrap();
        let skills = load(profile);
        let research: Vec<_> = skills.iter().filter(|s| s.name == "research").collect();
        assert_eq!(research.len(), 1);
        assert_eq!(research[0].body, "Mine.");
        assert!(!research[0].builtin);
        assert!(!skills.iter().any(|s| s.name == "broken"));
    }
}
