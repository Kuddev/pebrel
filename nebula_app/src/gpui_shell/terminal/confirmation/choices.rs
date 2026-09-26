//! Codex's displayed one-key choices. A question digit submits the current
//! answer itself; appending Enter could accidentally submit the next question.
use super::*;

type Choices = (Fingerprint, String, Vec<String>, Vec<String>);

pub(super) fn describe(
    screen: &str,
    program: &str,
    session_id: Option<&str>,
    mode: TermMode,
    waiting: bool,
) -> Option<Choices> {
    if !waiting
        || crate::ai_agents::AgentKind::parse(program) != Some(crate::ai_agents::AgentKind::Codex)
    {
        return None;
    }
    let mut context: Vec<String> = screen
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .take(24)
        .map(|line| line.trim().to_owned())
        .collect();
    context.reverse();
    let footer = context.last()?.to_ascii_lowercase();
    let question = footer.contains("tab to add notes") && footer.contains("enter to submit");
    let approval = footer == "press enter to confirm or esc to cancel";
    if !question && !approval {
        return None;
    }
    static ROW: OnceLock<Regex> = OnceLock::new();
    let row = ROW.get_or_init(|| Regex::new(r"^(?:[›❯>]\s*)?([1-9])\.\s+(.+)$").unwrap());
    static KEY: OnceLock<Regex> = OnceLock::new();
    let key = KEY.get_or_init(|| Regex::new(r"\(([a-z])\)$").unwrap());
    let mut labels = Vec::new();
    let mut replies = Vec::new();
    let mut first_row = None;
    for (ix, line) in context[..context.len() - 1].iter().enumerate() {
        let Some(captures) = row.captures(line) else { continue };
        let number = captures[1].parse::<usize>().ok()?;
        if number != labels.len() + 1 || labels.len() >= 9 {
            return None;
        }
        first_row.get_or_insert(ix);
        let label = captures[2].trim();
        if label.chars().count() > 240 {
            return None;
        }
        if question {
            // Codex's Other/notes path needs free text and cannot be answered
            // by selecting an advertised option. Keep that path in the terminal.
            if label.starts_with("Other") || label.starts_with("None of the above") {
                continue;
            }
            replies.push(number.to_string());
            labels.push(label.to_owned());
        } else {
            let shortcut = key.captures(label)?;
            replies.push(shortcut[1].to_owned());
            labels.push(label[..shortcut.get(0)?.start()].trim().to_owned());
        }
    }
    if labels.is_empty() {
        return None;
    }
    let first = first_row?;
    let question =
        context[..first].iter().rev().find(|line| !line.starts_with("Question "))?.clone();
    Some((
        Fingerprint {
            context,
            program: program.into(),
            session_id: session_id.map(str::to_owned),
            mode,
        },
        question,
        labels,
        replies,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    const QUESTION: &str = "Question 1/2 (2 unanswered)\nChoose a scope.\n› 1. Current  Selected directory\n  2. All  Whole workspace\ntab to add notes | enter to submit answer | ←/→ to navigate questions | esc to interrupt";

    #[test]
    fn a_number_submits_only_the_current_question_and_old_buttons_expire() {
        let mut state = ConfirmationState::default();
        let first = state.capture(QUESTION, "codex", Some("s"), TermMode::empty(), true).unwrap();
        assert_eq!(first.choices.len(), 2);
        assert_eq!(
            state.answer_choice(first.id, 1, QUESTION, "codex", Some("s"), TermMode::empty(), true),
            Some(b"2".to_vec())
        );
        assert!(
            state
                .answer_choice(first.id, 1, QUESTION, "codex", Some("s"), TermMode::empty(), true)
                .is_none()
        );
        let second = QUESTION.replace("Question 1/2", "Question 2/2");
        let current = state.capture(&second, "codex", Some("s"), TermMode::empty(), true).unwrap();
        assert_ne!(first.id, current.id);
        assert!(
            state
                .answer_choice(first.id, 0, &second, "codex", Some("s"), TermMode::empty(), true)
                .is_none()
        );
    }

    #[test]
    fn approval_uses_the_advertised_shortcut_and_rejects_unknown_controls() {
        let prompt = "Grant permissions?\n› 1. For this turn (y)\n2. Deny (d)\nPress enter to confirm or esc to cancel";
        let (_, _, labels, replies) =
            describe(prompt, "codex", None, TermMode::empty(), true).unwrap();
        assert_eq!(labels, ["For this turn", "Deny"]);
        assert_eq!(replies, ["y", "d"]);
        for invalid in [
            prompt.replace("(d)", "(ctrl+d)"),
            prompt.replace("2. Deny", "4. Deny"),
            prompt.replace("Press enter", "Documentation: press enter"),
        ] {
            assert!(describe(&invalid, "codex", None, TermMode::empty(), true).is_none());
        }
        assert!(describe(prompt, "claude", None, TermMode::empty(), true).is_none());
        assert!(describe(prompt, "codex", None, TermMode::empty(), false).is_none());
    }
}
