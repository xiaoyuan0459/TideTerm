//! Streaming legacy encodings and terminal-originated action policy.
use encoding_rs::{CoderResult, Decoder, Encoding, UTF_8};
use tiderm_core::session_options::SessionOptions;

pub struct SessionCodec {
    encoding: &'static Encoding,
    decoder: Decoder,
    escape: bool,
    osc: Vec<u8>,
    oversized: bool,
    last_bell: Option<std::time::Instant>,
}

impl SessionCodec {
    pub fn encoding(name: &str) -> Result<&'static Encoding, String> {
        let encoding = Encoding::for_label(name.as_bytes())
            .ok_or_else(|| "unsupported character set".to_string())?;
        if !encoding.is_ascii_compatible() {
            return Err("terminal character set must preserve ASCII controls".into());
        }
        Ok(encoding)
    }

    pub fn new(name: &str) -> Self {
        let encoding = Self::encoding(name).unwrap_or(UTF_8);
        Self {
            encoding,
            decoder: encoding.new_decoder_without_bom_handling(),
            escape: false,
            osc: Vec::new(),
            oversized: false,
            last_bell: None,
        }
    }

    pub fn encode(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        if self.encoding == UTF_8 {
            return Ok(bytes.to_vec());
        }
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Ok(bytes.to_vec());
        };
        let (encoded, _, errors) = self.encoding.encode(text);
        if errors {
            return Err("输入包含当前字符集无法表示的字符".into());
        }
        Ok(encoded.into_owned())
    }

    pub fn decode(&mut self, bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 3 + 16);
        let mut read = 0;
        loop {
            let (result, used, _) = self
                .decoder
                .decode_to_string(&bytes[read..], &mut out, false);
            read += used;
            if result == CoderResult::InputEmpty {
                return out;
            }
            out.reserve(bytes.len() * 3 + 16);
        }
    }

    /// Keep policy across packet boundaries. BEL inside OSC is a terminator, not a bell.
    pub fn filter(&mut self, text: &str, options: &SessionOptions) -> (Vec<u8>, bool) {
        let mut out = Vec::with_capacity(text.len());
        let mut bell = false;
        for &byte in text.as_bytes() {
            if !self.osc.is_empty() || self.oversized {
                let done = byte == 7 || (self.escape && byte == b'\\');
                if !self.oversized {
                    self.osc.push(byte);
                }
                self.escape = byte == 0x1b;
                if done {
                    let payload = self.osc.strip_prefix(b"\x1b]").unwrap_or_default();
                    let allowed = (options.remote_clipboard || !payload.starts_with(b"52;"))
                        && (options.remote_title
                            || !(payload.starts_with(b"0;") || payload.starts_with(b"2;")));
                    if !self.oversized && allowed {
                        out.extend_from_slice(&self.osc);
                    }
                    self.osc.clear();
                    self.escape = false;
                    self.oversized = false;
                } else if self.osc.len() > 16384 {
                    self.osc.clear();
                    self.oversized = true;
                }
                continue;
            }
            if self.escape {
                self.escape = false;
                if byte == b']' {
                    self.osc.extend_from_slice(b"\x1b]");
                    continue;
                }
                out.push(0x1b);
            }
            if byte == 0x1b {
                self.escape = true;
            } else if byte == 7 {
                let now = std::time::Instant::now();
                if self.last_bell.is_none_or(|last| {
                    now.duration_since(last).as_secs() >= u64::from(options.bell_interval_secs)
                }) {
                    self.last_bell = Some(now);
                    bell |= options.bell_sound;
                    if options.bell_visual {
                        out.push(byte);
                    }
                }
            } else {
                out.push(byte);
            }
        }
        (out, bell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_gbk_and_security_controls_preserve_terminal_bytes() {
        let mut codec = SessionCodec::new("GBK");
        assert_eq!(
            codec.encode("中文\r".as_bytes()).unwrap(),
            b"\xd6\xd0\xce\xc4\r"
        );
        assert_eq!(codec.decode(b"\xd6"), "");
        assert_eq!(codec.decode(b"\xd0\xce"), "中");
        assert_eq!(codec.decode(b"\xc4\r"), "文\r");
        let options = SessionOptions {
            remote_clipboard: false,
            remote_title: false,
            bell_visual: false,
            bell_sound: true,
            ..Default::default()
        };
        let mut actual = Vec::new();
        let mut bells = 0;
        for byte in b"a\x1b]52;c;c2VjcmV0\x1b\\b\x1b]2;bad-title\x07c\x07\x1b[6nd" {
            let (out, bell) = codec.filter(&String::from_utf8(vec![*byte]).unwrap(), &options);
            actual.extend(out);
            bells += u32::from(bell);
        }
        assert_eq!(actual, b"abc\x1b[6nd");
        assert_eq!(bells, 1);
        assert!(codec.encode("😀".as_bytes()).is_err());
    }
}
