use std::ops::RangeInclusive;

const UNCLOSED: &str = "A quoted value is missing its closing quote.";
const UNICODE_ESCAPE: &str = "A \\u escape needs four hexadecimal digits.";
const LONE_SURROGATE: &str = "A \\u escape names half of a character.";
const HIGH_SURROGATES: RangeInclusive<u32> = 0xD800..=0xDBFF;
const LOW_SURROGATES: RangeInclusive<u32> = 0xDC00..=0xDFFF;

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
            None => return Err(UNCLOSED.to_owned()),
            Some('"') => return Ok(text),
            Some('\\') => text.push(escaped(characters)?),
            Some(character) => text.push(character),
        }
    }
}

fn escaped(characters: &mut impl Iterator<Item = char>) -> Result<char, String> {
    match characters.next() {
        Some(escaped @ ('"' | '\\' | '/')) => Ok(escaped),
        Some('b') => Ok('\u{8}'),
        Some('f') => Ok('\u{c}'),
        Some('n') => Ok('\n'),
        Some('r') => Ok('\r'),
        Some('t') => Ok('\t'),
        Some('u') => unicode(characters),
        Some(other) => Err(format!("\\{other} isn't a JSON escape.")),
        None => Err(UNCLOSED.to_owned()),
    }
}

fn unicode(characters: &mut impl Iterator<Item = char>) -> Result<char, String> {
    let unit = code_unit(characters)?;
    if !HIGH_SURROGATES.contains(&unit) {
        return char::from_u32(unit).ok_or_else(|| LONE_SURROGATE.to_owned());
    }
    if characters.next() != Some('\\') || characters.next() != Some('u') {
        return Err(LONE_SURROGATE.to_owned());
    }
    let low = code_unit(characters)?;
    if !LOW_SURROGATES.contains(&low) {
        return Err(LONE_SURROGATE.to_owned());
    }
    let scalar =
        0x10000 + ((unit - HIGH_SURROGATES.start()) << 10) + (low - LOW_SURROGATES.start());
    char::from_u32(scalar).ok_or_else(|| LONE_SURROGATE.to_owned())
}

fn code_unit(characters: &mut impl Iterator<Item = char>) -> Result<u32, String> {
    let digits: String = characters.take(4).collect();
    if digits.len() != 4 || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
        return Err(UNICODE_ESCAPE.to_owned());
    }
    u32::from_str_radix(&digits, 16).map_err(|_| UNICODE_ESCAPE.to_owned())
}
