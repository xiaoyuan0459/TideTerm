//! A deliberately small argument-path grammar, never a shell evaluator.
#[derive(Clone)]
pub(super) struct Draft {
    pub head: String,
    pub token: String,
    dialect: u8,
    windows: bool,
}
fn simple(c: char, windows: bool) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':') || (windows && c == '\\')
}
pub(super) fn parse(query: &str, dialect: u8, windows: bool) -> Option<Draft> {
    if dialect > 2 || query.len() > 4096 || query.chars().any(char::is_control) {
        return None;
    }
    let mut chars = query.char_indices().peekable();
    let mut quoted = false;
    let mut start = 0;
    let mut token = String::new();
    while let Some((at, c)) = chars.next() {
        if dialect != 0 && c == '\'' {
            if dialect == 2 && quoted && chars.peek().is_some_and(|(_, c)| *c == '\'') {
                chars.next();
                token.push('\'');
            } else {
                quoted = !quoted;
            }
        } else if !quoted && c == ' ' {
            start = at + 1;
            token.clear();
        } else if dialect == 1 && !quoted && c == '\\' {
            if chars.next().is_none_or(|(_, c)| c != '\'') {
                return None;
            }
            token.push('\'');
        } else if quoted || simple(c, windows) {
            token.push(c);
        } else {
            return None;
        }
    }
    // Earlier words must remain simple, verbatim text. Quotes, substitutions,
    // operators and incomplete constructs before the argument are unsupported.
    let head = &query[..start];
    if token.is_empty() || !head.chars().all(|c| c == ' ' || simple(c, windows)) {
        return None;
    }
    Some(Draft {
        head: head.into(),
        token,
        dialect,
        windows,
    })
}
impl Draft {
    pub(super) fn command(&self, path: &str) -> Option<String> {
        let (token, quoted) = word(path, self.dialect, self.windows, false)?;
        if quoted && self.head.trim().is_empty() {
            return None;
        }
        let command = format!("{}{token}", self.head);
        (command.len() <= 4096).then_some(command)
    }
}
pub(super) fn invocation(path: &str, dialect: u8, windows: bool) -> Option<String> {
    let (token, _) = word(path, dialect, windows, dialect == 2)?;
    let command = if dialect == 2 {
        format!("& {token}")
    } else {
        token
    };
    (command.len() <= 4096).then_some(command)
}
pub(super) fn word(
    path: &str,
    dialect: u8,
    windows: bool,
    force_quote: bool,
) -> Option<(String, bool)> {
    if dialect > 2 || path.is_empty() {
        return None;
    }
    if path
        .chars()
        .any(|c| c.is_control() || (c.is_whitespace() && c != ' '))
    {
        return None;
    }
    // PowerShell wildcard parameters still interpret these inside quotes.
    // Universal wildcard/command-specific semantics need a separate contract.
    if dialect == 2 && path.contains(['*', '?', '[', ']', '‘', '’', '“', '”']) {
        return None;
    }
    let simple = path.chars().all(|c| simple(c, windows && dialect != 1));
    let token = if simple && !force_quote {
        path.to_owned()
    } else {
        if dialect == 0 {
            return None;
        }
        let escaped = match dialect {
            1 => path.replace('\'', "'\\''"),
            2 => path.replace('\'', "''"),
            _ => return None,
        };
        format!("'{escaped}'")
    };
    Some((token, !simple || force_quote))
}
