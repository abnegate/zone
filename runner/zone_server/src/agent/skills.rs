//! The skills index: what this workspace has written down about how particular
//! kinds of work are done, listed by name and trigger so a turn can tell which
//! one to read before it starts.
//!
//! A skill is a workspace document filed under [`SKILL_CATEGORY`], and its text
//! is a `SKILL.md` as CC and GK ship them: front matter whose `description`
//! says when the skill applies, then the procedure. The index carries only
//! that line for each skill — the name, the id `read_document` opens it by,
//! and the trigger — and `read_document` is the loader. So the prompt pays one
//! line per skill, and the model reads a procedure only when the work in front
//! of it is the kind the line describes (GK 801-810; CL 1464-1481, where
//! reading the relevant skill is the unconditional first step before producing
//! the thing it covers).
//!
//! How to choose one is CX 128-150 and CC 295-318: by name when asked for it,
//! from the trigger otherwise, never on a word alone, and when a skill is what
//! made you stop, say so.
//!
//! The lines are workspace-written text landing in the system prompt, and the
//! shape of a line is what keeps one from opening a section of its own: it
//! starts with a bullet, so it is no heading, and it ends with the id in
//! brackets, so it is no underline or break — a title has nowhere to be
//! either. The block above and below is the prompt's own text.
use crate::db::DbResult;
use crate::db::knowledge::{self, SkillRow};
use sqlx::PgPool;
use uuid::Uuid;

/// How many skills the index lists before it says how many it left out.
pub const MAX_SKILLS: usize = 40;

/// The most of a trigger line the index keeps. A trigger is a sentence or
/// two; past this it is the procedure, which the skill itself is for.
pub const MAX_TRIGGER_CHARS: usize = 160;

/// The whole block, lines and notice included, never exceeds this.
pub const MAX_RENDERED_BYTES: usize = 8_000;

const HEADING: &str = "\n\n# Skills";

/// The rule the index opens with. It names `read_document` because that is
/// the loader, and says what a skill outranks — the model's defaults, and not
/// the person's request — because a procedure the person did not ask to have
/// followed is still theirs to override.
const RULE: &str = "This workspace's written procedures, one line each: the name, the id read_document \
     opens it by, and when it applies. Before starting work a line describes, read that skill \
     in full and follow it; it outranks your own defaults and not the person's request. Use a \
     skill by name when asked for it, and otherwise by what its line says it is for, never on \
     a word alone. If a skill is what makes you stop or refuse, say which one.";

/// The trigger line of a skill, read from the head of its text.
///
/// Front matter first: the `description` a `SKILL.md` declares, quoted or
/// bare, on one line or as a folded block. A skill written without front
/// matter is indexed by its first line of prose, so a plain procedure still
/// says something about itself. Either way the result is one line, whitespace
/// collapsed and cut at [`MAX_TRIGGER_CHARS`].
pub fn trigger(head: &str) -> String {
    let described = front_matter_description(head)
        .or_else(|| first_prose_line(head))
        .unwrap_or_default();
    cut(&collapsed(&described))
}

/// The `description` value of a front-matter block, when the text opens with
/// one. A quoted value ends at its closing quote; a block scalar (`>` or `|`)
/// continues over the indented lines below it.
fn front_matter_description(head: &str) -> Option<String> {
    let mut lines = head.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut lines = lines.peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "---" {
            return None;
        }
        let Some(value) = line.strip_prefix("description:") else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || matches!(value, ">" | "|" | ">-" | "|-" | ">+" | "|+") {
            let mut folded = String::new();
            while let Some(next) = lines.peek() {
                if next.trim() == "---" || !next.starts_with([' ', '\t']) {
                    break;
                }
                folded.push_str(next.trim());
                folded.push(' ');
                lines.next();
            }
            return Some(folded);
        }
        if let Some(quote) = value
            .chars()
            .next()
            .filter(|mark| matches!(mark, '"' | '\''))
        {
            let inner = &value[quote.len_utf8()..];
            return Some(match inner.find(quote) {
                Some(end) => inner[..end].to_string(),
                None => inner.to_string(),
            });
        }
        return Some(value.to_string());
    }
    None
}

/// The first line that is prose rather than structure: past any front matter,
/// not a heading, not a fence or anything inside one, not blank.
fn first_prose_line(head: &str) -> Option<String> {
    let mut lines = head.lines().map(str::trim).peekable();
    if lines.peek() == Some(&"---") {
        lines.next();
        for line in lines.by_ref() {
            if line == "---" {
                break;
            }
        }
    }
    let mut fenced = false;
    for line in lines {
        if line.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || line.is_empty() || line.starts_with('#') {
            continue;
        }
        return Some(line.to_string());
    }
    None
}

fn collapsed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn cut(text: &str) -> String {
    if text.chars().count() <= MAX_TRIGGER_CHARS {
        return text.to_string();
    }
    let mut kept: String = text.chars().take(MAX_TRIGGER_CHARS - 1).collect();
    kept.truncate(kept.trim_end().len());
    kept.push('…');
    kept
}

/// One line of the index: the title, the id in brackets, the trigger. The
/// title and the trigger are collapsed to one line each, so a skill cannot
/// smuggle a second line into the block.
fn line(skill: &SkillRow) -> String {
    let title = collapsed(&skill.title);
    let trigger = trigger(&skill.head);
    if trigger.is_empty() {
        format!("- {title} [{}]\n", skill.id)
    } else {
        format!("- {title} [{}]: {trigger}\n", skill.id)
    }
}

fn omitted(count: usize) -> String {
    let noun = if count == 1 { "skill is" } else { "skills are" };
    format!(
        "{count} more {noun} filed under {} but not listed here; list_documents shows them.\n",
        knowledge::SKILL_CATEGORY
    )
}

/// The block, ready to append to a system prompt. Empty when the workspace
/// has no skills, so a reader without any pays nothing for the feature.
///
/// `filed` is how many skills the workspace has in all, which may exceed the
/// rows handed in. The bound is absolute: past [`MAX_SKILLS`] lines, or past
/// [`MAX_RENDERED_BYTES`], the index says how many it left out rather than
/// listing them, because an index that reads as complete and is not would
/// have the model believe a procedure does not exist.
pub fn render(skills: &[SkillRow], filed: usize) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let filed = filed.max(skills.len());
    let head = format!("{HEADING}\n{RULE}\n");
    let lines: Vec<String> = skills.iter().take(MAX_SKILLS).map(line).collect();
    let mut used = head.len();
    let mut kept = 0;
    for line in &lines {
        if used + line.len() > MAX_RENDERED_BYTES {
            break;
        }
        used += line.len();
        kept += 1;
    }
    if kept == filed {
        let mut rendered = format!("{head}{}", lines.concat());
        rendered.truncate(rendered.trim_end().len());
        return rendered;
    }
    while kept > 0 && used + omitted(filed - kept).len() > MAX_RENDERED_BYTES {
        kept -= 1;
        used -= lines[kept].len();
    }
    let mut rendered = format!("{head}{}{}", lines[..kept].concat(), omitted(filed - kept));
    rendered.truncate(rendered.trim_end().len());
    rendered
}

/// Every skill this workspace has filed, rendered for a reader that holds
/// `read_document`. The store reaches one row past the bound, and only when
/// that row exists is the workspace asked how many it has, so the notice can
/// state a count without the common case paying for one.
pub async fn prompt(pool: &PgPool, workspace_id: Uuid) -> DbResult<String> {
    let limit = i64::try_from(MAX_SKILLS).unwrap_or(i64::MAX);
    let skills = knowledge::skills(pool, workspace_id, limit).await?;
    let filed = if skills.len() > MAX_SKILLS {
        knowledge::skill_count(pool, workspace_id).await?
    } else {
        skills.len()
    };
    Ok(render(&skills, filed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(title: &str, head: &str) -> SkillRow {
        SkillRow {
            id: Uuid::new_v4(),
            title: title.to_string(),
            head: head.to_string(),
        }
    }

    #[test]
    fn a_skill_md_is_indexed_by_its_front_matter_description() {
        let head = "---\nname: deploy\ndescription: Use when shipping a release to production, before the first command.\nlicense: MIT\n---\n\n# Deploying\n\nStep one.";
        assert_eq!(
            trigger(head),
            "Use when shipping a release to production, before the first command."
        );
    }

    #[test]
    fn a_quoted_or_folded_description_is_read_whole() {
        let quoted = "---\ndescription: \"Use this skill whenever the user wants a Word document: create, read, or edit .docx\"\n---\nBody";
        assert_eq!(
            trigger(quoted),
            "Use this skill whenever the user wants a Word document: create, read, or edit .docx"
        );
        let folded = "---\nname: review\ndescription: >\n  Use when asked to review a pull request\n  or a diff, and never for writing one.\nversion: 2\n---\nBody";
        assert_eq!(
            trigger(folded),
            "Use when asked to review a pull request or a diff, and never for writing one."
        );
    }

    #[test]
    fn a_skill_without_front_matter_is_indexed_by_its_first_line_of_prose() {
        let head = "# House style for PHP\n\n```php\n<?php\n```\nEvery class is final unless it is a base class.\nMore.";
        assert_eq!(
            trigger(head),
            "Every class is final unless it is a base class."
        );
        assert_eq!(trigger("# Only a heading\n"), "");
        assert_eq!(trigger("---\nname: x\n---\n"), "");
    }

    #[test]
    fn a_trigger_is_one_line_and_bounded() {
        let head = format!("---\ndescription: {}\n---\n", "word ".repeat(80));
        let trigger = trigger(&head);
        assert_eq!(trigger.chars().count(), MAX_TRIGGER_CHARS);
        assert!(trigger.ends_with('…'), "{trigger}");
        assert!(!trigger.contains('\n'));
        assert!(!trigger.contains("  "));
    }

    #[test]
    fn a_workspace_with_no_skills_pays_nothing() {
        assert_eq!(render(&[], 0), "");
    }

    #[test]
    fn the_index_names_each_skill_with_its_id_and_trigger_under_the_rule() {
        let deploy = skill(
            "Deploy checklist",
            "---\ndescription: Use when shipping to production.\n---\n",
        );
        let php = skill(
            "PHP house style",
            "Every class is final unless it is a base class.",
        );
        let rendered = render(&[deploy.clone(), php.clone()], 2);
        assert!(rendered.starts_with("\n\n# Skills\n"), "{rendered}");
        assert!(rendered.contains("read_document"), "{rendered}");
        assert!(
            rendered.contains(&format!(
                "\n- Deploy checklist [{}]: Use when shipping to production.\n",
                deploy.id
            )),
            "{rendered}"
        );
        assert!(
            rendered.ends_with(&format!(
                "- PHP house style [{}]: Every class is final unless it is a base class.",
                php.id
            )),
            "{rendered}"
        );
        assert!(!rendered.contains("more skill"), "{rendered}");
    }

    #[test]
    fn past_the_bound_the_index_says_how_many_it_left_out() {
        let skills: Vec<SkillRow> = (0..MAX_SKILLS + 3)
            .map(|index| {
                skill(
                    &format!("Skill {index:02}"),
                    "---\ndescription: When.\n---\n",
                )
            })
            .collect();
        let rendered = render(&skills, skills.len());
        assert_eq!(rendered.matches("\n- Skill ").count(), MAX_SKILLS);
        assert!(
            rendered.ends_with("3 more skills are filed under skill but not listed here; list_documents shows them."),
            "{rendered}"
        );
        let one_over: Vec<SkillRow> = (0..MAX_SKILLS + 1)
            .map(|index| skill(&format!("Skill {index:02}"), ""))
            .collect();
        assert!(render(&one_over, one_over.len()).contains("1 more skill is filed"));
        // The store hands over one row past the bound; the count says the rest.
        assert!(
            render(&one_over, MAX_SKILLS + 7).contains("7 more skills are filed"),
            "the notice states what is filed, not what was fetched"
        );
    }

    #[test]
    fn the_bytes_are_bounded_however_long_the_titles_are() {
        let skills: Vec<SkillRow> = (0..MAX_SKILLS)
            .map(|index| {
                skill(
                    &format!("{index} {}", "title ".repeat(60)),
                    &"w".repeat(200),
                )
            })
            .collect();
        let rendered = render(&skills, skills.len());
        assert!(rendered.len() <= MAX_RENDERED_BYTES, "{}", rendered.len());
        assert!(rendered.contains("more skills are filed"), "{rendered}");
    }

    /// A title is one line however it was written, and a line that starts
    /// with a bullet and ends with the id is neither a heading nor a break.
    #[test]
    fn a_title_cannot_add_a_line_or_open_a_section() {
        let rendered = render(
            &[
                skill("# Rules\nfor everyone", ""),
                skill("---", "---\ndescription: ==\n---\n"),
            ],
            2,
        );
        let lines: Vec<&str> = rendered.lines().filter(|line| line.contains('[')).collect();
        assert_eq!(lines.len(), 2, "{rendered}");
        assert!(
            lines[0].starts_with("- # Rules for everyone ["),
            "{rendered}"
        );
        assert!(
            lines[1].starts_with("- --- [") && lines[1].ends_with("]: =="),
            "{rendered}"
        );
    }
}
