//! The text of the spec and the guide. Embedded in the binary and returned by the MCP tool `spec`.

/// One document: its id and its text. An id without a directory prefix of `guide/` is a path relative to the crate's
/// `spec/` (a copy of the workspace's `spec/`), without the extension; `guide/<name>` is read from the crate's `guide/`
/// (a copy of the workspace's `guide/`).
macro_rules! parts {
    (spec: [$($id:literal),* $(,)?], guide: [$($gid:literal),* $(,)?]) => {
        &[
            $(($id, include_str!(concat!("../../spec/", $id, ".md")))),*,
            $((concat!("guide/", $gid), include_str!(concat!("../../guide/", $gid, ".md")))),*
        ]
    };
}

/// The list of embedded spec documents, in reading order.
pub const PARTS: &[(&str, &str)] = parts!(
    spec: [
    "README",
    "language/01-lexical",
    "language/02-lines",
    "language/03-statements",
    "language/04-labels-modules",
    "language/05-sugar",
    "language/06-diagnostics",
    "language/07-migrate",
    "language/08-format",
    "host/01-execution",
    "host/02-registry",
    "host/03-packages",
    "host/04-save",
    "host/05-texts",
    "host/06-cli",
    "host/07-repl",
    "host/08-test",
    "host/09-debug",
    "host/10-bevy",
    "packages/audio",
    "packages/event",
    "packages/json",
    "packages/flow",
    "packages/image",
    "packages/input",
    "packages/math",
    "packages/screen",
    "packages/text",
    ],
    guide: [
    "README",
    "01-getting-started",
    "02-script-basics",
    "03-embedding",
    "04-save-and-resume",
    "05-waiting-for-events",
    "06-json",
    "07-running-safely",
    "08-test-and-debug",
    ]
);

/// The first heading of the text (empty if none).
pub fn title(text: &str) -> &str {
    text.lines()
        .find_map(|l| l.strip_prefix("# "))
        .unwrap_or("")
        .trim()
}

/// Finds the text by name (`language/01-lexical`).
pub fn find(id: &str) -> Option<&'static str> {
    PARTS.iter().find(|(i, _)| *i == id).map(|(_, t)| *t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_spec_texts_are_embedded_with_headings() {
        for (id, text) in PARTS {
            assert!(!text.is_empty(), "{id}");
            assert!(!title(text).is_empty(), "{id} に見出しがない");
        }
        assert!(find("language/01-lexical").is_some());
        assert!(find("nothing").is_none());
    }

    #[test]
    fn all_spec_documents_in_directory_are_listed() {
        let top = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = top.join("spec");
        let mut on_disk = Vec::new();
        for entry in std::fs::read_dir(top.join("guide")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "md") {
                let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                on_disk.push(format!("guide/{stem}"));
            }
        }
        for dir in ["", "language", "host", "packages"] {
            for entry in std::fs::read_dir(root.join(dir)).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_some_and(|e| e == "md") {
                    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                    on_disk.push(if dir.is_empty() {
                        stem
                    } else {
                        format!("{dir}/{stem}")
                    });
                }
            }
        }
        on_disk.sort();
        let mut listed: Vec<String> = PARTS.iter().map(|(i, _)| i.to_string()).collect();
        listed.sort();
        assert_eq!(on_disk, listed);
    }
}
