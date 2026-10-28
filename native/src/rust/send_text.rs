//! Bounded UTF-8 editing contract. Conversion never writes to a transport.
use crate::bridge::multiline::multiline_ffi::{SendOptions, SendPreview};

pub(super) const MAX_BYTES: usize = 64 << 10;
const MAX_LINES: usize = 2048;

pub(super) fn convert(text: &str, options: &SendOptions) -> Result<Vec<Vec<u8>>, u8> {
    if text.is_empty() {
        return Err(2);
    }
    if text.len() > MAX_BYTES {
        return Err(3);
    }
    let eol = match options.eol {
        0 => "\r",
        1 => "\n",
        2 => "\r\n",
        _ => return Err(4),
    };
    if !matches!(options.tabs, 0 | 2 | 4 | 8) {
        return Err(4);
    }
    let mut normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    if options.tabs != 0 {
        normalized = normalized.replace('\t', &" ".repeat(options.tabs as usize));
    }
    if options.append && !normalized.ends_with('\n') {
        normalized.push('\n');
    }
    let mut lines = Vec::new();
    let mut size = 0;
    for line in normalized.split_inclusive('\n') {
        let bytes = if let Some(body) = line.strip_suffix('\n') {
            [body.as_bytes(), eol.as_bytes()].concat()
        } else {
            line.as_bytes().to_vec()
        };
        size += bytes.len();
        if size > MAX_BYTES || lines.len() >= MAX_LINES {
            return Err(3);
        }
        lines.push(bytes);
    }
    Ok(lines)
}

pub fn multiline_preview(text: &str, options: &SendOptions) -> SendPreview {
    match convert(text, options) {
        Ok(lines) => {
            let bytes = lines.iter().map(Vec::len).sum::<usize>() as u32;
            let mut visible = String::new();
            for line in &lines {
                for ch in String::from_utf8_lossy(line).chars() {
                    match ch {
                        '\r' => visible.push_str("\\r"),
                        '\n' => visible.push_str("\\n"),
                        '\t' => visible.push_str("\\t"),
                        c if c.is_control() => visible.push_str(&format!("\\u{{{:x}}}", c as u32)),
                        c => visible.push(c),
                    }
                }
                visible.push('\n');
            }
            SendPreview {
                text: visible,
                bytes,
                lines: lines.len() as u32,
                error: 0,
            }
        }
        Err(error) => SendPreview {
            text: String::new(),
            bytes: 0,
            lines: 0,
            error,
        },
    }
}
