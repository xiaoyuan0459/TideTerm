//! Lightweight semantic accents on visible spans only. Explicit ANSI colors win.
//! No retained document, per-character regex, or complete history scan.
pub fn accent_visible(records: &mut Vec<u32>, cw: u32, body_x: u32, palette: [[u8; 3]; 16]) {
    let rgba = |rgb: [u8; 3]| u32::from_le_bytes([rgb[0], rgb[1], rgb[2], 255]);
    let default = rgba(palette[7]);
    let mut token = String::new();
    let mut positions = Vec::new();
    let mut last = None;
    let mut underlines = Vec::new();
    let mut expect_command = false;
    let mut flush = |records: &mut Vec<u32>,
                     token: &mut String,
                     positions: &mut Vec<usize>,
                     command: &mut bool| {
        let link = token.starts_with("https://")
            || token.starts_with("http://")
            || token.starts_with("ftp://");
        let path = token.as_bytes().get(1) == Some(&b':')
            && token
                .as_bytes()
                .get(2)
                .is_some_and(|b| *b == b'\\' || *b == b'/')
            || token.starts_with("~/")
            || token.starts_with('/');
        let number = !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit() || b == b'.');
        let color = if link || (*command && token.chars().next().is_some_and(|c| c.is_alphabetic()))
        {
            Some(rgba(palette[14]))
        } else if path || number {
            Some(rgba(palette[11]))
        } else {
            None
        };
        if let Some(color) = color {
            for &i in positions.iter() {
                if records[i + 3] == default {
                    records[i + 3] = color;
                }
            }
            if link
                && positions.iter().all(|&i| records[i + 3] == color)
                && let (Some(&first), Some(&end)) = (positions.first(), positions.last())
            {
                let x = records[first + 1] & 0xffff;
                let y = (records[first + 1] >> 16).saturating_add(2).min(0xffff);
                let width = (records[end + 1] & 0xffff) - x + cw;
                underlines.extend_from_slice(&[0, x | y << 16, width | 1 << 16, color, 0, 0]);
            }
        }
        *command = token.ends_with(['>', '#', '$']);
        token.clear();
        positions.clear();
    };
    for i in (0..records.len()).step_by(6) {
        if records[i] & 255 != 1 {
            continue;
        }
        let x = records[i + 1] & 0xffff;
        let y = records[i + 1] >> 16;
        if x < body_x {
            continue;
        }
        if last.is_some_and(|(px, py)| y != py || x != px + cw) {
            flush(records, &mut token, &mut positions, &mut expect_command);
            if last.is_some_and(|(_, py)| py != y) {
                expect_command = false;
            }
        }
        if let Some(c) = char::from_u32(records[i + 4]) {
            token.push(c);
            positions.push(i);
        }
        last = Some((x, y));
    }
    flush(records, &mut token, &mut positions, &mut expect_command);
    records.extend(underlines);
}
