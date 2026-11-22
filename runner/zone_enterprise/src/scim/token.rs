#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Token {
    Word(String),
    Text(String),
    OpenBracket,
    CloseBracket,
    OpenParenthesis,
    CloseParenthesis,
}

impl Token {
    pub(crate) fn is_keyword(&self, keyword: &str) -> bool {
        matches!(self, Self::Word(word) if word.eq_ignore_ascii_case(keyword))
    }
}

pub(crate) fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut characters = input.chars().peekable();
    while let Some(&character) = characters.peek() {
        match character {
            _ if character.is_whitespace() => {
                characters.next();
            }
            '[' | ']' | '(' | ')' => {
                characters.next();
                tokens.push(match character {
                    '[' => Token::OpenBracket,
                    ']' => Token::CloseBracket,
                    '(' => Token::OpenParenthesis,
                    _ => Token::CloseParenthesis,
                });
            }
            '"' => {
                characters.next();
                tokens.push(Token::Text(quoted(&mut characters)?));
            }
            _ => {
                let mut word = String::new();
                while let Some(&next) = characters.peek() {
                    if next.is_whitespace() || matches!(next, '"' | '[' | ']' | '(' | ')') {
                        break;
                    }
                    word.push(next);
                    characters.next();
                }
                tokens.push(Token::Word(word));
            }
        }
    }
    Ok(tokens)
}

fn quoted(characters: &mut impl Iterator<Item = char>) -> Result<String, String> {
    let mut text = String::new();
    loop {
        match characters.next() {
            None => return Err("A quoted value is missing its closing quote.".to_owned()),
            Some('"') => return Ok(text),
            Some('\\') => match characters.next() {
                Some(escaped @ ('"' | '\\' | '/')) => text.push(escaped),
                Some(other) => {
                    return Err(format!(
                        "\\{other} isn't a supported escape; only \\\", \\\\ and \\/ are."
                    ));
                }
                None => return Err("A quoted value is missing its closing quote.".to_owned()),
            },
            Some(character) => text.push(character),
        }
    }
}
