pub(super) const OLD_PREFIX: &str = "a/";
pub(super) const NEW_PREFIX: &str = "b/";

const NO_FILE: &str = "/dev/null";
const QUOTE: u8 = b'"';
const SPACE: u8 = b' ';
const ESCAPE: u8 = b'\\';
const NAME_END: char = '\t';
const QUOTED_NEW: &str = " \"b/";
const UNQUOTED_NEW: &str = " b/";

pub(super) fn header_paths(text: &str) -> Option<(String, String)> {
    if text.as_bytes().first() == Some(&QUOTE) {
        let (old, rest) = quoted(text)?;
        let new = rest.strip_prefix(char::from(SPACE))?;
        return Some((
            without(&old, OLD_PREFIX),
            without(&unquote(new), NEW_PREFIX),
        ));
    }
    if let Some(index) = text.find(QUOTED_NEW) {
        return Some((
            without(&text[..index], OLD_PREFIX),
            without(&unquote(&text[index + 1..]), NEW_PREFIX),
        ));
    }
    if let Some(path) = symmetric(text) {
        return Some((path.to_string(), path.to_string()));
    }
    let index = text.find(UNQUOTED_NEW)?;
    Some((
        without(&text[..index], OLD_PREFIX),
        without(&text[index + 1..], NEW_PREFIX),
    ))
}

pub(super) fn side(text: &str, prefix: &str) -> Option<String> {
    let name = if text.as_bytes().first() == Some(&QUOTE) {
        quoted(text).map_or_else(|| text.to_string(), |(name, _)| name)
    } else {
        text.split(NAME_END).next().unwrap_or_default().to_string()
    };
    (name != NO_FILE).then(|| without(&name, prefix))
}

pub(super) fn unquote(text: &str) -> String {
    match quoted(text) {
        Some((name, "")) => name,
        _ => text.to_string(),
    }
}

fn symmetric(text: &str) -> Option<&str> {
    let middle = text.len().checked_sub(1)? / 2;
    if text.len().is_multiple_of(2) || text.as_bytes().get(middle) != Some(&SPACE) {
        return None;
    }
    let old = text.get(..middle)?.strip_prefix(OLD_PREFIX)?;
    let new = text.get(middle + 1..)?.strip_prefix(NEW_PREFIX)?;
    (old == new).then_some(old)
}

fn without(name: &str, prefix: &str) -> String {
    name.strip_prefix(prefix).unwrap_or(name).to_string()
}

fn quoted(text: &str) -> Option<(String, &str)> {
    let bytes = text.as_bytes();
    if bytes.first() != Some(&QUOTE) {
        return None;
    }
    let mut name = Vec::with_capacity(bytes.len());
    let mut index = 1;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            QUOTE => {
                return Some((
                    String::from_utf8_lossy(&name).into_owned(),
                    &text[index + 1..],
                ));
            }
            ESCAPE => {
                let (decoded, width) = escaped(&bytes[index + 1..])?;
                name.push(decoded);
                index += 1 + width;
            }
            _ => {
                name.push(byte);
                index += 1;
            }
        }
    }
    None
}

fn escaped(bytes: &[u8]) -> Option<(u8, usize)> {
    let first = *bytes.first()?;
    let decoded = match first {
        b'a' => 0x07,
        b'b' => 0x08,
        b't' => b'\t',
        b'n' => b'\n',
        b'v' => 0x0b,
        b'f' => 0x0c,
        b'r' => b'\r',
        b'0'..=b'7' => return octal(bytes),
        other => other,
    };
    Some((decoded, 1))
}

fn octal(bytes: &[u8]) -> Option<(u8, usize)> {
    let digits = bytes.get(..3)?;
    let mut value: u32 = 0;
    for digit in digits {
        if !(b'0'..=b'7').contains(digit) {
            return None;
        }
        value = value * 8 + u32::from(digit - b'0');
    }
    Some((u8::try_from(value).ok()?, 3))
}
