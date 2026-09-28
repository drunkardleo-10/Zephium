//! Settings for skills: the built-in ones and the person's own, which live as
//! `<name>/SKILL.md` folders in the profile's skills directory. A person's
//! skill of a built-in's name replaces it; a turned-off skill is listed in
//! `.disabled` there and never offered to the lead.
use std::path::{Path, PathBuf};

use zephium_core::ids::ProfileId;
use zephium_core::work::model::WorkModelRole;

use crate::work_lead::skills::{self as lead, Skill, SkillFault, MAX_USER_SKILLS};

const DISABLED: &str = ".disabled";

/// One skill as Settings lists it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkillRow {
    pub name: String,
    pub description: String,
    /// Ships with the app and has no version of the person's.
    pub builtin: bool,
    /// The person's own version of a built-in.
    pub customized: bool,
    pub enabled: bool,
    pub tools: Vec<String>,
    pub role: Option<WorkModelRole>,
}

/// Why a change to skills was refused, in closed words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkillChangeFault {
    /// The text is not a skill: which part is wrong.
    Text(SkillFault),
    NotFound,
    /// A built-in can be turned off or customised, not deleted.
    BuiltIn,
    /// Another skill of the person's already has this name.
    Taken,
    Full,
    Unavailable,
}

fn dir(profile: ProfileId) -> Result<PathBuf, SkillChangeFault> {
    lead::user_dir(profile).ok_or(SkillChangeFault::Unavailable)
}

/// Names the person turned off; the lead leaves them out.
pub fn disabled(profile: ProfileId) -> Vec<String> {
    lead::user_dir(profile)
        .map(|dir| disabled_in(&dir))
        .unwrap_or_default()
}

fn disabled_in(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(DISABLED))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|name| lead::valid_name(name))
        .map(str::to_owned)
        .collect()
}

fn write_disabled(dir: &Path, names: &[String]) -> Result<(), SkillChangeFault> {
    std::fs::create_dir_all(dir).map_err(|_| SkillChangeFault::Unavailable)?;
    let mut text = names.join("\n");
    text.push('\n');
    atomic_write(&dir.join(DISABLED), &text)
}

fn atomic_write(path: &Path, text: &str) -> Result<(), SkillChangeFault> {
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, text).map_err(|_| SkillChangeFault::Unavailable)?;
    std::fs::rename(&temporary, path).map_err(|_| {
        let _ = std::fs::remove_file(&temporary);
        SkillChangeFault::Unavailable
    })
}

/// Built-ins and the person's own, by name; the person's replace built-ins.
pub fn list(profile: ProfileId) -> Vec<SkillRow> {
    lead::user_dir(profile)
        .map(|dir| list_in(&dir))
        .unwrap_or_default()
}

fn list_in(dir: &Path) -> Vec<SkillRow> {
    let off = disabled_in(dir);
    let builtins = lead::builtins();
    let mine = lead::read_dir(dir);
    let mut rows: Vec<SkillRow> = builtins
        .iter()
        .filter(|skill| !mine.iter().any(|own| own.name == skill.name))
        .chain(mine.iter())
        .map(|skill| SkillRow {
            name: skill.name.clone(),
            description: skill.description.clone(),
            builtin: skill.builtin,
            customized: !skill.builtin && builtins.iter().any(|b| b.name == skill.name),
            enabled: !off.contains(&skill.name),
            tools: skill.tools.clone(),
            role: skill.role,
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

fn role_name(role: WorkModelRole) -> &'static str {
    match role {
        WorkModelRole::Lead => "lead",
        WorkModelRole::Page => "page",
        WorkModelRole::Light => "light",
        _ => "lead",
    }
}

/// A skill as its `SKILL.md` text.
pub fn render(skill: &Skill) -> String {
    let mut text = format!(
        "---\nname: {}\ndescription: {}\n",
        skill.name, skill.description
    );
    if !skill.tools.is_empty() {
        text.push_str(&format!("tools: [{}]\n", skill.tools.join(", ")));
    }
    if let Some(role) = skill.role {
        text.push_str(&format!("role: {}\n", role_name(role)));
    }
    text.push_str("---\n\n");
    text.push_str(&skill.body);
    text.push('\n');
    text
}

/// The text of a skill: the person's file as written, or a built-in's.
pub fn read(profile: ProfileId, name: &str) -> Result<(String, bool), SkillChangeFault> {
    read_in(&dir(profile)?, name)
}

fn read_in(root: &Path, name: &str) -> Result<(String, bool), SkillChangeFault> {
    if !lead::valid_name(name) {
        return Err(SkillChangeFault::NotFound);
    }
    let path = root.join(name).join("SKILL.md");
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_file() && meta.len() <= lead::MAX_SKILL_BYTES as u64 {
            let text = std::fs::read_to_string(&path).map_err(|_| SkillChangeFault::Unavailable)?;
            return Ok((text, false));
        }
    }
    lead::builtins()
        .iter()
        .find(|skill| skill.name == name)
        .map(|skill| (render(skill), true))
        .ok_or(SkillChangeFault::NotFound)
}

/// Writes the person's skill. `previous` is the name it had, when it is
/// renamed; saving under a built-in's name customises that built-in.
pub fn save(
    profile: ProfileId,
    previous: Option<&str>,
    text: &str,
) -> Result<SkillRow, SkillChangeFault> {
    save_in(&dir(profile)?, previous, text)
}

fn save_in(root: &Path, previous: Option<&str>, text: &str) -> Result<SkillRow, SkillChangeFault> {
    let skill = lead::parse(text).map_err(SkillChangeFault::Text)?;
    let mine = lead::read_dir(root);
    let renamed = previous.filter(|previous| *previous != skill.name);
    if mine.iter().any(|own| own.name == skill.name) && (renamed.is_some() || previous.is_none()) {
        return Err(SkillChangeFault::Taken);
    }
    if !mine.iter().any(|own| own.name == skill.name) && mine.len() >= MAX_USER_SKILLS {
        return Err(SkillChangeFault::Full);
    }
    let folder = root.join(&skill.name);
    std::fs::create_dir_all(&folder).map_err(|_| SkillChangeFault::Unavailable)?;
    atomic_write(
        &folder.join("SKILL.md"),
        text.trim_start_matches('\u{feff}'),
    )?;
    if let Some(previous) = renamed.filter(|name| lead::valid_name(name)) {
        let _ = std::fs::remove_dir_all(root.join(previous));
        let mut off = disabled_in(root);
        if let Some(at) = off.iter().position(|name| name == previous) {
            off[at] = skill.name.clone();
            write_disabled(root, &off)?;
        }
    }
    list_in(root)
        .into_iter()
        .find(|row| row.name == skill.name)
        .ok_or(SkillChangeFault::Unavailable)
}

/// Deletes the person's skill; a customised built-in goes back to the app's.
pub fn delete(profile: ProfileId, name: &str) -> Result<(), SkillChangeFault> {
    delete_in(&dir(profile)?, name)
}

fn delete_in(root: &Path, name: &str) -> Result<(), SkillChangeFault> {
    if !lead::valid_name(name) {
        return Err(SkillChangeFault::NotFound);
    }
    let folder = root.join(name);
    if !folder.join("SKILL.md").is_file() {
        return Err(if lead::builtins().iter().any(|skill| skill.name == name) {
            SkillChangeFault::BuiltIn
        } else {
            SkillChangeFault::NotFound
        });
    }
    std::fs::remove_dir_all(&folder).map_err(|_| SkillChangeFault::Unavailable)?;
    if !lead::builtins().iter().any(|skill| skill.name == name) {
        let mut off = disabled_in(root);
        if off.iter().any(|known| known == name) {
            off.retain(|known| known != name);
            write_disabled(root, &off)?;
        }
    }
    Ok(())
}

pub fn set_enabled(profile: ProfileId, name: &str, enabled: bool) -> Result<(), SkillChangeFault> {
    set_enabled_in(&dir(profile)?, name, enabled)
}

fn set_enabled_in(root: &Path, name: &str, enabled: bool) -> Result<(), SkillChangeFault> {
    if !list_in(root).iter().any(|row| row.name == name) {
        return Err(SkillChangeFault::NotFound);
    }
    let mut off = disabled_in(root);
    off.retain(|known| known != name);
    if !enabled {
        off.push(name.to_owned());
        off.sort();
    }
    write_disabled(root, &off)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEEKLY: &str =
        "---\nname: weekly-review\ndescription: Review my week from my calendar and notes.\n---\n\n# Weekly review\n";

    #[test]
    fn a_persons_skills_are_written_renamed_turned_off_and_deleted() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        assert!(list_in(root).iter().all(|row| row.builtin));
        let row = save_in(root, None, WEEKLY).unwrap();
        assert_eq!(row.name, "weekly-review");
        assert!(row.enabled && !row.builtin && !row.customized);
        assert_eq!(save_in(root, None, WEEKLY), Err(SkillChangeFault::Taken));
        assert!(matches!(
            save_in(root, None, "no frontmatter"),
            Err(SkillChangeFault::Text(SkillFault::NoFrontmatter))
        ));
        set_enabled_in(root, "weekly-review", false).unwrap();
        assert_eq!(disabled_in(root), ["weekly-review"]);
        let renamed = save_in(
            root,
            Some("weekly-review"),
            &WEEKLY.replace("weekly-review", "friday-review"),
        )
        .unwrap();
        assert!(!renamed.enabled);
        assert_eq!(disabled_in(root), ["friday-review"]);
        assert!(read_in(root, "weekly-review").is_err());
        let (text, builtin) = read_in(root, "friday-review").unwrap();
        assert!(text.contains("name: friday-review") && !builtin);
        delete_in(root, "friday-review").unwrap();
        assert!(disabled_in(root).is_empty());
        assert_eq!(
            delete_in(root, "friday-review"),
            Err(SkillChangeFault::NotFound)
        );
        assert_eq!(delete_in(root, "../etc"), Err(SkillChangeFault::NotFound));
    }

    #[test]
    fn a_skill_renders_back_to_text_it_parses_from() {
        let skill = lead::parse(
            "---\nname: trip-planning\ndescription: Plan a trip.\ntools: [web_search, start_part]\nrole: light\n---\n\nParts: Stay, Flights, Entry.",
        )
        .unwrap();
        assert_eq!(lead::parse(&render(&skill)).unwrap(), skill);
    }
}
