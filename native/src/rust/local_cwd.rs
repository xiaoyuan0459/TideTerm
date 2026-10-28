//! Decode a completed directory notification. Stream framing belongs to DirectoryContext.
use std::path::Path;

pub(super) fn from_osc(payload: &str) -> Option<String> {
    let path = if let Some(raw) = payload.strip_prefix("9;9;") {
        raw.trim().trim_matches('"').to_owned()
    } else {
        let uri = url::Url::parse(payload.strip_prefix("7;")?.trim()).ok()?;
        if uri.scheme() != "file"
            || !uri.username().is_empty()
            || uri.password().is_some()
            || uri.port().is_some()
            || uri.query().is_some()
            || uri.fragment().is_some()
            || uri
                .host_str()
                .is_some_and(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost"))
        {
            return None;
        }
        let decoded = percent_encoding::percent_decode_str(uri.path())
            .decode_utf8()
            .ok()?
            .into_owned();
        #[cfg(windows)]
        {
            let bytes = decoded.as_bytes();
            if bytes.len() >= 3
                && bytes[0] == b'/'
                && bytes[1].is_ascii_alphabetic()
                && (bytes[2] == b'/' || bytes[2] == b':')
            {
                let tail = if bytes[2] == b':' {
                    &decoded[3..]
                } else {
                    &decoded[2..]
                };
                format!(
                    "{}:{}",
                    char::from(bytes[1].to_ascii_uppercase()),
                    tail.replace('/', "\\")
                )
            } else {
                decoded
            }
        }
        #[cfg(not(windows))]
        {
            decoded
        }
    };
    (!path.is_empty()
        && path.len() <= 16384
        && !path.contains(['\0', '\r', '\n'])
        && Path::new(&path).is_absolute())
    .then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_notifications_decode_only_local_absolute_paths() {
        #[cfg(windows)]
        for (input, expected) in [
            ("7;file:///c:/fixture/a%20b", "C:\\fixture\\a b"),
            (
                "7;file:///c/fixture/%E4%B8%AD%E6%96%87",
                "C:\\fixture\\中文",
            ),
            ("9;9;\"C:\\fixture\\a b\"", "C:\\fixture\\a b"),
        ] {
            assert_eq!(from_osc(input).as_deref(), Some(expected));
        }
        #[cfg(not(windows))]
        for (input, expected) in [
            ("7;file:///tmp/a%20b", "/tmp/a b"),
            ("7;file:///a/b", "/a/b"),
        ] {
            assert_eq!(from_osc(input).as_deref(), Some(expected));
        }
        for invalid in [
            "0;title",
            "7;file://untrusted-host/share",
            "7;https://localhost/path",
            "9;9;relative",
            "7;file:///tmp/%00",
            "7;file:///tmp/%0a",
            "7;file:///tmp/%FF",
            "7;file:///tmp/a?secret=value",
        ] {
            assert!(from_osc(invalid).is_none());
        }
    }
}
