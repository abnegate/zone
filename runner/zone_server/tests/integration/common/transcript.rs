//! A prompt a coding agent CLI read on stdin, split back into the blocks the
//! transcript renders, and the shapes a turn's prompt is expected to take.

pub const NOW: &str = "- Now: ";
pub const SEARCH_OPEN: &str = "<web_search_context>";
pub const SEARCH_CLOSE: &str = "</web_search_context>";

/// Who a block of a rendered transcript is from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    System,
    User,
    Assistant,
    Tool,
}

impl Speaker {
    const ALL: [Self; 4] = [Self::System, Self::User, Self::Assistant, Self::Tool];

    fn heading(self) -> &'static str {
        match self {
            Self::System => "System:",
            Self::User => "User:",
            Self::Assistant => "Assistant:",
            Self::Tool => "Tool result:",
        }
    }

    fn opening(line: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|speaker| speaker.heading() == line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub speaker: Speaker,
    pub content: String,
}

impl Block {
    /// Whether the block is the search state the server hands each turn.
    pub fn searched(&self) -> bool {
        self.speaker == Speaker::User
            && self.content.starts_with(SEARCH_OPEN)
            && self.content.ends_with(SEARCH_CLOSE)
    }
}

/// `prompt` split back into the blocks the transcript renders: a heading line
/// that opens the prompt or follows a blank line starts the next one.
pub fn blocks(prompt: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut lines: Vec<&str> = Vec::new();
    let mut speaker = None;
    let mut previous = "";
    for (index, line) in prompt.lines().enumerate() {
        match Speaker::opening(line).filter(|_| index == 0 || previous.is_empty()) {
            Some(opened) => {
                if let Some(speaker) = speaker {
                    blocks.push(Block {
                        speaker,
                        content: lines.join("\n").trim().to_string(),
                    });
                }
                speaker = Some(opened);
                lines.clear();
            }
            None => {
                assert!(
                    speaker.is_some(),
                    "the prompt must open with a speaker: {prompt}"
                );
                lines.push(line);
            }
        }
        previous = line;
    }
    if let Some(speaker) = speaker {
        blocks.push(Block {
            speaker,
            content: lines.join("\n").trim().to_string(),
        });
    }
    blocks
}

/// `content` without the search state it carries, and whether it carried one.
pub fn unsearched(content: &str) -> (String, bool) {
    match content.split_once(SEARCH_OPEN) {
        Some((before, after)) => {
            let (_, after) = after
                .split_once(SEARCH_CLOSE)
                .unwrap_or_else(|| panic!("an unterminated search state: {content}"));
            (format!("{before}{after}"), true)
        }
        None => (content.to_string(), false),
    }
}

/// Asserts `prompt` is the whole conversation: the instructions, then exactly
/// `exchanges` in order, beside this turn's search state.
pub fn assert_replays(prompt: &str, exchanges: &[(Speaker, &str)]) {
    let blocks = blocks(prompt);
    let Some((instructions, conversation)) = blocks.split_first() else {
        panic!("an empty prompt replays nothing");
    };
    assert_eq!(
        instructions.speaker,
        Speaker::System,
        "a replay opens with the instructions: {prompt}"
    );
    assert!(
        instructions.content.contains(NOW) && instructions.content.lines().count() > 1,
        "a replay carries the whole system prompt, not a note: {prompt}"
    );
    let (searches, conversation): (Vec<&Block>, Vec<&Block>) =
        conversation.iter().partition(|block| block.searched());
    assert!(
        searches.len() <= 1,
        "a turn carries its own search state once: {prompt}"
    );
    let conversation: Vec<(Speaker, &str)> = conversation
        .iter()
        .map(|block| (block.speaker, block.content.as_str()))
        .collect();
    assert_eq!(
        conversation, exchanges,
        "a replay carries every exchange, in order: {prompt}"
    );
}

/// Asserts `prompt` is only what a resumed session has not yet seen: an
/// optional note holding this turn's clock, then `message`. This turn's search
/// state may ride in the note or follow the message, once.
pub fn assert_resumes_with(prompt: &str, message: &str) {
    let blocks = blocks(prompt);
    let (note, rest) = match blocks.split_first() {
        Some((first, rest)) if first.speaker == Speaker::System => (Some(first), rest),
        _ => (None, blocks.as_slice()),
    };
    let mut searches = 0;
    if let Some(note) = note {
        let (clock, searched) = unsearched(&note.content);
        searches += usize::from(searched);
        let lines: Vec<&str> = clock
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        assert!(
            matches!(lines.as_slice(), [only] if only.starts_with(NOW)),
            "an unchanged prompt is not sent again; the note holds the clock alone: {prompt}"
        );
    }
    let rest = match rest {
        [message, search] if search.searched() => {
            searches += 1;
            std::slice::from_ref(message)
        }
        rest => rest,
    };
    assert!(
        searches <= 1,
        "a turn carries its own search state once: {prompt}"
    );
    assert_eq!(
        rest,
        [Block {
            speaker: Speaker::User,
            content: message.to_string(),
        }],
        "a resumed turn sends only the new message: {prompt}"
    );
}
