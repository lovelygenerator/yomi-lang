//! Conversion between `file://` URIs and paths (including percent-encoding).

use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;

/// The path from a `file://` URI. Other kinds of URIs are `None`.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let rest = uri.as_str().strip_prefix("file://")?;
    // Skip the authority (host name) (`file://localhost/…`)
    let path = &rest[rest.find('/')?..];
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let text = String::from_utf8(out).ok()?;
    Some(PathBuf::from(if cfg!(windows) {
        windows_path_from_uri_path(&text)
    } else {
        text
    }))
}

/// The `file://` URI from a path.
pub fn path_to_uri(path: &Path) -> Option<Uri> {
    let text = path.to_str()?;
    let mut out = String::from("file://");
    if cfg!(windows) {
        out.push_str(&uri_path_from_windows_path(text));
    } else {
        out.push_str(&percent_encode(text));
    }
    Uri::from_str(&out).ok()
}

fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The part starting with a drive letter (`C:`).
fn drive_letter(text: &str) -> Option<char> {
    let b = text.as_bytes();
    (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':').then_some(b[0] as char)
}

/// From a Windows path (`C:\a b\x`) to the path part of a URI (`/C:/a%20b/x`).
/// The `:` of the drive letter is kept as is (`file:///C:/…`).
fn uri_path_from_windows_path(text: &str) -> String {
    let text = text.replace('\\', "/");
    match drive_letter(&text) {
        Some(drive) => format!("/{drive}:{}", percent_encode(&text[2..])),
        None => percent_encode(&text),
    }
}

/// From a decoded URI path (`/c:/a b/x`) to a Windows path (`C:\a b\x`).
/// Editors sometimes send the drive letter in lowercase, so it is made uppercase.
fn windows_path_from_uri_path(path: &str) -> String {
    let rest = path.strip_prefix('/').unwrap_or(path);
    match drive_letter(rest) {
        Some(drive) => format!(
            "{}:{}",
            drive.to_ascii_uppercase(),
            rest[2..].replace('/', "\\")
        ),
        None => path.replace('/', "\\"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_uris_round_trip_with_japanese_and_spaces() {
        let path = Path::new("/tmp/台本 1/main.yomi");
        let uri = path_to_uri(path).unwrap();
        assert_eq!(uri.as_str(), "file:///tmp/%E5%8F%B0%E6%9C%AC%201/main.yomi");
        assert_eq!(uri_to_path(&uri).as_deref(), Some(path));
    }

    #[test]
    fn windows_paths_become_uri_paths_keeping_drive_letter() {
        assert_eq!(
            uri_path_from_windows_path("C:\\Users\\yoshi\\台本 1\\main.yomi"),
            "/C:/Users/yoshi/%E5%8F%B0%E6%9C%AC%201/main.yomi"
        );
    }

    #[test]
    fn uri_path_returns_to_windows_path_with_uppercase_drive_letter() {
        // The decoded form (`/c:/…`) of `file:///c%3A/Users/x/main.yomi` sent by editors is the same
        assert_eq!(
            windows_path_from_uri_path("/c:/Users/x/a b/main.yomi"),
            "C:\\Users\\x\\a b\\main.yomi"
        );
        assert_eq!(
            windows_path_from_uri_path(&uri_path_from_windows_path("C:\\a\\b.yomi")),
            "C:\\a\\b.yomi"
        );
    }

    #[cfg(windows)]
    #[test]
    fn paths_and_uris_round_trip_on_windows() {
        let path = Path::new("C:\\Users\\yoshi\\台本 1\\main.yomi");
        let uri = path_to_uri(path).unwrap();
        assert_eq!(
            uri.as_str(),
            "file:///C:/Users/yoshi/%E5%8F%B0%E6%9C%AC%201/main.yomi"
        );
        assert_eq!(uri_to_path(&uri).as_deref(), Some(path));
    }

    #[test]
    fn non_file_uri_is_not_a_path() {
        let uri = Uri::from_str("untitled:Untitled-1").unwrap();
        assert_eq!(uri_to_path(&uri), None);
    }
}
