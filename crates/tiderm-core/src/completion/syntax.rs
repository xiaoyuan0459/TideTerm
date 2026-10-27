//! Conservative single-line token positions for reference completion only.
/// Recognize the bounded literal names that the app can insert without quoting.
pub fn literal_command_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(b))
}
/// Split simple literal words and preserve byte positions. Refuses operators,
/// substitutions, unfinished quoting, controls and drafts larger than 4096 bytes.
pub fn simple_command_words(text: &str) -> Option<(Vec<(String, usize)>, bool)> {
    if text.len() > 4096 || text.chars().any(char::is_control) {
        return None;
    }
    let mut result = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut start = None;
    for (at, c) in text.char_indices() {
        if start.is_none() && !c.is_whitespace() {
            start = Some(at);
        }
        if escaped {
            word.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if q == '"' && matches!(c, '$' | '`') {
                return None;
            }
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            ';' | '|' | '&' | '<' | '>' | '`' | '$' | '(' | ')' | '{' | '}' => return None,
            c if c.is_whitespace() => {
                if let Some(at) = start.take() {
                    result.push((std::mem::take(&mut word), at));
                }
            }
            _ => word.push(c),
        }
    }
    if quote.is_some() || escaped {
        return None;
    }
    let separated = start.is_none();
    if let Some(at) = start {
        result.push((word, at));
    }
    Some((result, separated))
}
