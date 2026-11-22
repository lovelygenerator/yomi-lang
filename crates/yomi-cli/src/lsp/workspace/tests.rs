use super::*;

const MAIN: &str = "\
import std::flow

identity 案内人 = \"案内人\"

save var 信頼度: int = 0

fn double(n: int) -> int {
  return n * 2
}

label start {
  let a: int = double(2)
  信頼度 += a
  案内人「こんにちは」
  jump next
}

label next {
  @flow.wait(1.0)
  end
}
";

fn workspace() -> (Workspace, PathBuf) {
    let path = PathBuf::from("/virtual/main.yomi");
    let mut ws = Workspace::new(
        Some(PathBuf::from("/virtual")),
        crate::session::registry(),
        Localizer::from_env(&crate::LOCALES, Some("en")),
    );
    ws.set_document(path.clone(), MAIN.to_string());
    (ws, path)
}

fn pos_of(needle: &str, nth: usize) -> Position {
    let mut seen = 0;
    for (i, line) in MAIN.lines().enumerate() {
        let mut from = 0;
        while let Some(at) = line[from..].find(needle) {
            let byte = from + at;
            if seen == nth {
                let col = line[..byte].encode_utf16().count() as u32;
                return Position::new(i as u32, col + 1);
            }
            seen += 1;
            from = byte + needle.len();
        }
    }
    panic!("{needle} がない");
}

#[test]
fn diagnostics_are_empty_for_valid_scripts_and_per_file_for_broken_ones() {
    let (mut ws, path) = workspace();
    let ok = ws.diagnostics();
    assert_eq!(ok.len(), 1);
    assert!(
        ok[0]
            .1
            .iter()
            .all(|d| d.severity != Some(DiagnosticSeverity::ERROR)),
        "{:?}",
        ok[0].1
    );
    ws.set_document(path, "label start {\n  jump nowhere\n}\n".into());
    let bad = ws.diagnostics();
    let err = bad[0]
        .1
        .iter()
        .find(|d| d.severity == Some(DiagnosticSeverity::ERROR))
        .expect("エラーがある");
    assert_eq!(err.range.start.line, 1);
    assert_eq!(err.source.as_deref(), Some("yomi"));
    assert!(err.message.contains("nowhere"), "{}", err.message);
}

#[test]
fn definition_jumps_to_label_variable_and_function_declarations() {
    let (mut ws, path) = workspace();
    // next in `jump next` → `label next`
    let at = pos_of("next", 0);
    let target = ws.definition(&path, at).expect("定義がある");
    assert_eq!(target.range.start.line, 17);
    // `double(2)` → `fn double`
    let target = ws.definition(&path, pos_of("double", 1)).unwrap();
    assert_eq!(target.range.start.line, 6);
    // Even on the declaration, returns the declaration
    let target = ws.definition(&path, pos_of("信頼度", 0)).unwrap();
    assert_eq!(target.range.start.line, 4);
}

#[test]
fn references_can_include_or_exclude_declaration() {
    let (mut ws, path) = workspace();
    let at = pos_of("信頼度", 0);
    assert_eq!(ws.references(&path, at, false).len(), 1);
    assert_eq!(ws.references(&path, at, true).len(), 2);
}

#[test]
fn hover_returns_declaration_descriptions_command_signatures_and_keywords() {
    let (mut ws, path) = workspace();
    let text = |h: Hover| match h.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("markup"),
    };
    let h = text(ws.hover(&path, pos_of("double", 1)).unwrap());
    assert!(h.contains("fn double(n: int) -> int"), "{h}");
    let h = text(ws.hover(&path, pos_of("flow.wait", 0)).unwrap());
    assert!(h.contains("@flow.wait("), "{h}");
    let h = text(ws.hover(&path, pos_of("jump", 0)).unwrap());
    assert!(h.contains("jump (statement)"), "{h}");
    assert!(ws.hover(&path, Position::new(1, 1)).is_none());
}

#[test]
fn completion_candidates_depend_on_context() {
    let (mut ws, path) = workspace();
    let labels = |items: &[CompletionItem]| -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    };
    let mut set = |text: &str| {
        ws.set_document(path.clone(), format!("{MAIN}{text}"));
        let last = text.lines().last().unwrap_or("");
        let line = MAIN.lines().count() as u32 + text.lines().count() as u32 - 1;
        ws.completion(
            &path,
            Position::new(line, last.encode_utf16().count() as u32),
        )
    };
    let items = labels(&set("label x {\n  jump "));
    assert!(items.contains(&"next".to_string()) && items.contains(&"start".to_string()));
    assert!(!items.contains(&"if".to_string()));
    let items = labels(&set("label y {\n  @"));
    assert!(items.contains(&"flow.wait".to_string()));
    let items = labels(&set("label z {\n  flow."));
    assert!(items.contains(&"wait".to_string()), "{items:?}");
    let items = labels(&set("label w {\n  let q: int = 1\n  "));
    assert!(items.contains(&"if".to_string()));
    assert!(items.contains(&"q".to_string()), "ローカルが出る");
    assert!(items.contains(&"double".to_string()));
    let items = labels(&set("import "));
    assert!(items.contains(&"std::flow".to_string()), "{items:?}");
}

#[test]
fn multiple_modules_give_definitions_and_references_in_other_files() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main.yomi");
    let other = dir.path().join("story.yomi");
    std::fs::write(&other, "label intro {\n  end\n}\n").unwrap();
    let mut ws = Workspace::new(
        Some(dir.path().to_path_buf()),
        crate::session::registry(),
        Localizer::from_env(&crate::LOCALES, Some("en")),
    );
    ws.set_document(
        main.clone(),
        "import story\n\nlabel start {\n  call story.intro()\n  end\n}\n".into(),
    );
    let at = Position::new(3, 14);
    let def = ws.definition(&main, at).expect("定義がある");
    assert!(
        def.uri.as_str().ends_with("/story.yomi"),
        "{}",
        def.uri.as_str()
    );
    assert_eq!(def.range.start.line, 0);
    let refs = ws.references(&main, at, true);
    assert_eq!(refs.len(), 2);
}

#[test]
fn rename_rewrites_declaration_and_all_uses() {
    let (mut ws, path) = workspace();
    let range = ws
        .prepare_rename(&path, pos_of("信頼度", 1))
        .expect("名前の上");
    assert_eq!(range.start.line, 12);
    let edit = ws.rename(&path, pos_of("信頼度", 0), "親密度").unwrap();
    let edits: Vec<_> = edit
        .changes
        .iter()
        .flat_map(|c| c.values())
        .flatten()
        .collect();
    assert_eq!(edits.len(), 2);
    assert!(edits.iter().all(|e| e.new_text == "親密度"));
    assert!(
        ws.rename(&path, pos_of("信頼度", 0), "if").is_err(),
        "予約語"
    );
    assert!(
        ws.rename(&path, pos_of("信頼度", 0), "1a").is_err(),
        "数字で始まる"
    );
    assert!(
        ws.rename(&path, Position::new(1, 0), "x").is_err(),
        "名前の上ではない"
    );
}

#[test]
fn rename_also_rewrites_variables_before_fields_and_methods() {
    let src = "\
struct P { name: str, xs: list<int> }

label start {
  let p = P { name: \"a\", xs: [] }
  ${p.name}
  p.xs.push(1)
  let n = p.xs.len()
  end
}
";
    let path = PathBuf::from("/virtual/main.yomi");
    let mut ws = Workspace::new(
        Some(PathBuf::from("/virtual")),
        crate::session::registry(),
        Localizer::from_env(&crate::LOCALES, Some("en")),
    );
    ws.set_document(path.clone(), src.to_string());
    let edit = ws.rename(&path, Position::new(3, 7), "q").unwrap();
    let mut at: Vec<(u32, u32)> = edit
        .changes
        .iter()
        .flat_map(|c| c.values())
        .flatten()
        .map(|e| (e.range.start.line, e.range.start.character))
        .collect();
    at.sort();
    assert_eq!(at, [(3, 6), (4, 4), (5, 2), (6, 10)]);
}

#[test]
fn declaration_tree_and_workspace_search() {
    let (mut ws, path) = workspace();
    let symbols = ws.document_symbols(&path);
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        ["案内人", "信頼度", "start", "next", "double"]
            .iter()
            .copied()
            .filter(|n| names.contains(n))
            .collect::<Vec<_>>()
    );
    assert!(names.contains(&"start") && names.contains(&"double"));
    let found = ws.workspace_symbols("ST");
    assert!(found.iter().any(|s| s.name == "start"));
    assert!(!found.iter().any(|s| s.name == "next"));
    let highlights = ws.highlights(&path, pos_of("信頼度", 0));
    assert_eq!(highlights.len(), 2);
}

#[test]
fn completion_after_value_lists_fields_and_methods_of_declared_type() {
    let mut ws = Workspace::new(
        Some(PathBuf::from("/virtual")),
        crate::session::registry(),
        Localizer::from_env(&crate::LOCALES, Some("en")),
    );
    let path = PathBuf::from("/virtual/t.yomi");
    let head = "struct Item { name: str, qty: int }\nstruct Bag { items: list<Item>, owner: Item }\n\nlabel start {\n  let bag: Bag = Bag { items: [], owner: Item { name: \"a\", qty: 1 } }\n";
    let labels = |ws: &mut Workspace, tail: &str| -> Vec<String> {
        ws.set_document(path.clone(), format!("{head}{tail}"));
        let line = head.matches('\n').count() as u32;
        let col = tail.encode_utf16().count() as u32;
        ws.completion(&path, Position::new(line, col))
            .into_iter()
            .map(|i| i.label)
            .collect()
    };
    let fields = labels(&mut ws, "  bag.");
    assert!(
        fields.contains(&"items".to_string()) && fields.contains(&"owner".to_string()),
        "{fields:?}"
    );
    let nested = labels(&mut ws, "  bag.owner.");
    assert!(
        nested.contains(&"qty".to_string()) && !nested.contains(&"items".to_string()),
        "{nested:?}"
    );
    let list = labels(&mut ws, "  bag.items.");
    assert!(
        list.contains(&"push".to_string()) && list.contains(&"len".to_string()),
        "{list:?}"
    );
    assert!(labels(&mut ws, "  unknown.").is_empty());
}

#[test]
fn untyped_let_gets_completion_and_hover_with_inferred_type() {
    let mut ws = Workspace::new(
        Some(PathBuf::from("/virtual")),
        crate::session::registry(),
        Localizer::from_env(&crate::LOCALES, Some("en")),
    );
    let path = PathBuf::from("/virtual/u.yomi");
    let head = "struct Item { name: str, qty: int }\n\nlabel start {\n  let item = Item { name: \"a\", qty: 1 }\n  let xs = [item]\n";
    let at = |ws: &mut Workspace, tail: &str| {
        ws.set_document(path.clone(), format!("{head}{tail}"));
        (
            head.matches('\n').count() as u32,
            tail.encode_utf16().count() as u32,
        )
    };
    let (line, col) = at(&mut ws, "  item.");
    let labels: Vec<String> = ws
        .completion(&path, Position::new(line, col))
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(labels.contains(&"qty".to_string()), "{labels:?}");
    let (line, col) = at(&mut ws, "  xs.");
    let labels: Vec<String> = ws
        .completion(&path, Position::new(line, col))
        .into_iter()
        .map(|i| i.label)
        .collect();
    assert!(labels.contains(&"push".to_string()), "{labels:?}");
    let hover = ws.hover(&path, Position::new(3, 7)).expect("宣言の上");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("markup")
    };
    assert!(m.value.contains("item: Item"), "{}", m.value);
}
