//! Checks that the list of diagnostic codes in the spec (sections 6.5 to 6.9 of `spec/language/06-diagnostics.md`)
//! exactly matches the core's `all_codes()` (`DiagCode::ALL` and the codes of each kind).
//!
//! The spec files are relative to the workspace root (`CARGO_MANIFEST_DIR/../../spec/…`). If the files are missing (such
//! as a public repository exported with only the spec), it finishes without checking anything.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use yomi_core::{CodeGroup, Severity, all_codes};

const SPEC: &str = "spec/language/06-diagnostics.md";

/// One row of a table in the spec (code, argument names, severity).
#[derive(Debug, PartialEq, Eq)]
struct Row {
    args: BTreeSet<String>,
    severity: String,
}

/// Extracts all `` `name` `` in a cell.
fn backticked(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cell;
    while let Some(i) = rest.find('`') {
        let after = &rest[i + 1..];
        let Some(j) = after.find('`') else { break };
        out.push(after[..j].to_string());
        rest = &after[j + 1..];
    }
    out
}

/// Extracts argument names from an argument cell. The cell has the form `name` (note), `name`…, and `` ` `` inside notes
/// (in full-width parentheses) are not counted as names.
fn arg_names(cell: &str) -> BTreeSet<String> {
    let mut depth = 0u32;
    let mut top = String::new();
    for c in cell.chars() {
        match c {
            '（' | '(' => depth += 1,
            '）' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => top.push(c),
            _ => {}
        }
    }
    backticked(&top)
        .into_iter()
        .filter(|a| !a.is_empty())
        .collect()
}

/// Splits a row of a markdown table into cells (removing the leading and trailing `|`; assumes there is no `|` inside
/// `` ` ``).
fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// Reads the table after `<!-- codes:group -->`, per group.
fn parse(spec: &str) -> BTreeMap<String, BTreeMap<String, Row>> {
    let mut out: BTreeMap<String, BTreeMap<String, Row>> = BTreeMap::new();
    let mut group: Option<String> = None;
    let mut in_table = false;
    for line in spec.lines() {
        let t = line.trim();
        if let Some(g) = t
            .strip_prefix("<!-- codes:")
            .and_then(|r| r.strip_suffix("-->"))
        {
            group = Some(g.trim().to_string());
            in_table = false;
            out.entry(g.trim().to_string()).or_default();
            continue;
        }
        let Some(g) = &group else { continue };
        if t.starts_with('|') {
            in_table = true;
            let c = cells(t);
            // Header and separator
            if c.first()
                .is_some_and(|x| x == "コード" || x.starts_with("---"))
            {
                continue;
            }
            assert!(c.len() >= 5, "{SPEC}: 列が足りない行: {t}");
            let code = backticked(&c[0]);
            assert_eq!(code.len(), 1, "{SPEC}: コードのセルが正しくない: {t}");
            let args = arg_names(&c[1]);
            let row = Row {
                args,
                severity: c[2].clone(),
            };
            let prev = out.get_mut(g).unwrap().insert(code[0].clone(), row);
            assert!(prev.is_none(), "{SPEC}: {g}: {} が 2 度ある", code[0]);
        } else if in_table {
            // The end of the table
            group = None;
            in_table = false;
        }
    }
    out
}

fn severity_word(s: Severity) -> &'static str {
    match s {
        Severity::Error => "エラー",
        _ => "警告",
    }
}

#[test]
fn spec_code_list_exactly_matches_implementation() {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "..", "..", SPEC]
        .iter()
        .collect();
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("{} がないので、確かめない", path.display());
        return;
    };
    let spec = parse(&text);
    let mut problems: Vec<String> = Vec::new();
    for g in CodeGroup::ALL {
        let rows = spec.get(g.name());
        let Some(rows) = rows else {
            problems.push(format!("仕様書に組 {} の表がない", g.name()));
            continue;
        };
        let code: BTreeMap<&str, _> = all_codes()
            .into_iter()
            .filter(|c| c.group == *g)
            .map(|c| (c.code, c))
            .collect();
        for name in rows.keys() {
            if !code.contains_key(name.as_str()) {
                problems.push(format!(
                    "{}: 仕様書にあって、コードにない: {name}",
                    g.name()
                ));
            }
        }
        for (name, c) in &code {
            let Some(row) = rows.get(*name) else {
                problems.push(format!(
                    "{}: コードにあって、仕様書にない: {name}",
                    g.name()
                ));
                continue;
            };
            let want: BTreeSet<String> = c.args.iter().map(|a| (*a).to_string()).collect();
            if row.args != want {
                problems.push(format!(
                    "{}: {name}: 引数が違う（仕様書 {:?}、コード {:?}）",
                    g.name(),
                    row.args,
                    want
                ));
            }
            if row.severity != severity_word(c.severity) {
                problems.push(format!(
                    "{}: {name}: 重大度が違う（仕様書 {}、コード {}）",
                    g.name(),
                    row.severity,
                    severity_word(c.severity)
                ));
            }
        }
    }
    for g in spec.keys() {
        if !CodeGroup::ALL.iter().any(|x| x.name() == g) {
            problems.push(format!("仕様書に、知らない組 {g} がある"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn table_read_treats_no_args_and_none_as_empty_and_only_piped_lines_as_table() {
    let t = "<!-- codes:diag -->\n| コード | 引数 | 重大度 | 章 | 意味 |\n| --- | --- | --- | --- | --- |\n| `a_b` | なし | エラー | 1 | x |\n| `c` | `x`（文字列）、`y` | 警告 | 2 | y |\n\n文章\n| `z` | なし | エラー | 3 | 表の外 |\n";
    let p = parse(t);
    let rows = &p["diag"];
    assert_eq!(rows.len(), 2);
    assert!(rows["a_b"].args.is_empty());
    assert_eq!(rows["c"].args.len(), 2);
    assert_eq!(rows["c"].severity, "警告");
}

#[test]
fn spec_core_command_error_code_table_matches_core_command_error_codes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../spec/host/02-registry.md");
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let mut doc: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with('#') {
            inside = line.contains("コアのコマンドエラーコード");
        }
        if !inside || !line.trim_start().starts_with('|') {
            continue;
        }
        let c = cells(line);
        let code = backticked(&c[0]);
        if c.len() < 3 || code.len() != 1 {
            continue;
        }
        let args = if c[1] == "—" {
            BTreeSet::new()
        } else {
            backticked(&c[1]).into_iter().collect()
        };
        assert!(doc.insert(code[0].clone(), args).is_none(), "重複");
    }
    let actual: BTreeMap<String, BTreeSet<String>> = yomi_core::core_command_error_codes()
        .iter()
        .map(|c| {
            (
                c.code.to_string(),
                c.args.iter().map(|a| a.to_string()).collect(),
            )
        })
        .collect();
    assert_eq!(doc, actual);
}
