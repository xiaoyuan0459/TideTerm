//! Bounded OSC envelope. Reports are removed before decoding, logs and rendering.
const PREFIX: &[u8] = b"\x1b]777;tideterm-env;";
pub(super) const PATH_LIMIT: usize = 8192;
const WIRE_LIMIT: usize = PATH_LIMIT * 2 + 128;

#[derive(Default)]
pub(super) struct Wire {
    prefix: Vec<u8>,
    body: Vec<u8>,
    report: bool,
    escaped: bool,
    overflow: bool,
}
impl Wire {
    pub fn feed(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
        if self.prefix.is_empty() && !self.report && !bytes.contains(&27) {
            return (bytes.to_vec(), Vec::new());
        }
        let mut output = Vec::with_capacity(bytes.len());
        let mut reports = Vec::new();
        for &b in bytes {
            // An ESC other than ST aborts an OSC, as it does in the terminal
            // parser. A malformed report must not swallow subsequent CSI output.
            if self.report && self.escaped && b != b'\\' && b != 7 {
                self.report = false;
                self.overflow = false;
                self.escaped = false;
                self.body.clear();
                self.prefix.push(27);
            }
            if self.report {
                let done = b == 7 || (self.escaped && b == b'\\');
                if done {
                    if self.escaped {
                        self.body.pop();
                    }
                    if !self.overflow && reports.len() < 4 {
                        reports.push(std::mem::take(&mut self.body));
                    }
                    self.body.clear();
                    self.report = false;
                    self.overflow = false;
                    self.escaped = false;
                } else {
                    if self.body.len() < WIRE_LIMIT {
                        self.body.push(b);
                    } else {
                        self.overflow = true;
                    }
                    self.escaped = b == 27;
                }
                continue;
            }
            self.prefix.push(b);
            if PREFIX.starts_with(&self.prefix) {
                if self.prefix.len() == PREFIX.len() {
                    self.prefix.clear();
                    self.report = true;
                }
            } else {
                // Keep a trailing ESC as a possible start of the next envelope.
                let keep = b == 27;
                output.extend_from_slice(&self.prefix[..self.prefix.len() - usize::from(keep)]);
                self.prefix.clear();
                if keep {
                    self.prefix.push(b);
                }
            }
        }
        (output, reports)
    }
}

pub(super) fn command(nonce: &str, dialect: u8) -> Option<String> {
    match dialect {
        // Bash builtins only; C locale makes substring indexing byte-oriented.
        1 => Some(format!(
            r#"(LC_ALL=C; p=${{PATH-}}; if [[ ${{#p}} -le 8192 ]]; then builtin printf '\033]777;tideterm-env;1;{nonce};b;'; for ((i=0;i<${{#p}};i++)); do builtin printf '%02x' "'${{p:i:1}}"; done; builtin printf '\007'; fi)"#
        )),
        2 => Some(format!(
            r#"& {{ if ($env:PATH.Length -le 8192) {{ $t=[Text.Encoding]::UTF8.GetBytes([string]$env:PATH); if ($t.Length -le 8192) {{ $s=if ([IO.Path]::PathSeparator -eq ';') {{'w'}} else {{'p'}}; [Console]::Write(([char]27+']777;tideterm-env;1;{nonce};'+$s+';'+[BitConverter]::ToString($t).Replace('-','')+[char]7)) }} }} }}"#
        )),
        _ => None,
    }
}

pub(super) fn decode(body: &[u8], nonce: &str, dialect: u8) -> Option<(String, u8)> {
    let text = std::str::from_utf8(body).ok()?;
    let mut parts = text.splitn(4, ';');
    if parts.next()? != "1" || parts.next()? != nonce {
        return None;
    }
    let platform = match (dialect, parts.next()?) {
        (1, "b") => 1,
        (2, "w") => 2,
        (2, "p") => 3,
        _ => return None,
    };
    let path = decode_text(parts.next()?, PATH_LIMIT)?;
    if path.chars().any(char::is_control)
        || path.split(if platform == 2 { ';' } else { ':' }).count() > 128
    {
        return None;
    }
    Some((path, platform))
}

pub(super) fn decode_text(hex: &str, limit: usize) -> Option<String> {
    if hex.len() > limit * 2 || !hex.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |b: u8| (b as char).to_digit(16).map(|n| n as u8);
    let bytes = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| Some(nibble(b[0])? * 16 + nibble(b[1])?))
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}
