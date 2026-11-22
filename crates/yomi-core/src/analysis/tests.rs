use super::*;

const MAIN: &str = "\
import story::ch1 as c

identity 案内人 = \"案内人\"

save var 信頼度: int = 0

struct Point { x: int, y: int }

enum Shape { Dot, Circle(int) }

fn double(n: int) -> int {
  return n * 2
}

label start(count: int) {
  let a: int = double(count)
  信頼度 += a
  案内人「こんにちは」
  jump next
}

label next {
  let p: Point = Point { x: 1, y: 2 }
  call c.intro()
  match Shape.Dot {
    Dot => end
    Circle(r) => ${r}
  }
}
";

const CH1: &str = "label intro {\n  序章\n  end\n}\n";

fn find(a: &Analysis, kind: SymbolKind, name: &str) -> usize {
    a.symbols
        .iter()
        .position(|s| s.kind == kind && s.name == name)
        .unwrap_or_else(|| panic!("{name} がない"))
}

fn analysis() -> Analysis {
    analyze(&[("main", MAIN), ("story::ch1", CH1)])
}

#[test]
fn top_level_declarations_become_symbols_with_kind_and_description() {
    let a = analysis();
    let start = &a.symbols[find(&a, SymbolKind::Label, "start")];
    assert_eq!(start.detail, "label start(count: int)");
    assert_eq!(start.module, "main");
    let double = &a.symbols[find(&a, SymbolKind::Fn, "double")];
    assert_eq!(double.detail, "fn double(n: int) -> int");
    assert_eq!(
        a.symbols[find(&a, SymbolKind::Variant, "Circle")].detail,
        "Shape.Circle(int)"
    );
    assert_eq!(
        a.symbols[find(&a, SymbolKind::Field, "x")]
            .container
            .as_deref(),
        Some("Point")
    );
    assert_eq!(
        a.symbols[find(&a, SymbolKind::Var, "信頼度")].detail,
        "var 信頼度: int"
    );
}

#[test]
fn references_resolve_to_declarations() {
    let a = analysis();
    let count = |kind, name| {
        let id = find(&a, kind, name);
        a.references_to(id).count()
    };
    assert_eq!(count(SymbolKind::Label, "next"), 1, "jump");
    assert_eq!(count(SymbolKind::Fn, "double"), 1);
    assert_eq!(count(SymbolKind::Var, "信頼度"), 1);
    assert_eq!(count(SymbolKind::Identity, "案内人"), 1);
    assert_eq!(
        count(SymbolKind::Struct, "Point"),
        2,
        "型の注釈と struct のリテラル"
    );
    assert_eq!(count(SymbolKind::Field, "x"), 1);
    assert_eq!(count(SymbolKind::Variant, "Dot"), 2, "式とパターン");
    assert_eq!(count(SymbolKind::Enum, "Shape"), 1);
    assert_eq!(count(SymbolKind::Param, "count"), 1);
    assert_eq!(count(SymbolKind::Local, "a"), 1);
    assert_eq!(count(SymbolKind::Local, "r"), 1);
}

#[test]
fn label_of_another_module_resolved_through_aliased_call() {
    let a = analysis();
    let intro = find(&a, SymbolKind::Label, "intro");
    assert_eq!(a.symbols[intro].module, "story::ch1");
    let refs: Vec<_> = a.references_to(intro).collect();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].module, "main");
    // The `intro` in `  call c.intro()`
    assert_eq!(
        (refs[0].span.line, refs[0].span.col, refs[0].span.len),
        (24, 10, 5)
    );
}

#[test]
fn symbol_at_returns_same_symbol_from_declaration_and_use() {
    let a = analysis();
    let next = find(&a, SymbolKind::Label, "next");
    let decl = a.symbols[next].span;
    assert_eq!(a.symbol_at("main", decl.line, decl.col), Some(next));
    let r = a.references_to(next).next().unwrap().span;
    assert_eq!(a.symbol_at("main", r.line, r.col + 1), Some(next));
    assert_eq!(a.symbol_at("main", 1, 1), None);
}

#[test]
fn broken_scripts_still_return_symbols_of_parsed_parts() {
    let a = analyze(&[(
        "main",
        "label start {\n  jump nowhere\n  ???\n}\nlabel other {\n  end\n}\n",
    )]);
    assert!(a.symbols.iter().any(|s| s.name == "other"));
    assert_eq!(a.references.len(), 0);
}

const LIB: &str = "\
identity 門番 = \"門番\"

save var flag: bool = false

struct Item { name: str }

enum Mood { Calm, Angry(int) }

label greet() -> int {
  return 1
}
";

const USE: &str = "\
import lib

save var n: int = 0

label start {
  let items: list<lib.Item> = [lib.Item { name: \"a\" }]
  for it in items {
    ${it.name}
  }
  while n < 3 {
    n += 1
  }
  if lib.flag {
    let got = call lib.greet()
    n += got
  } else {
    lib.flag = true
  }
  let m = lib.Mood.Calm
  match m {
    Angry(level) => ${level}
    _ => end
  }
  lib.門番「やあ」
  choice {
    「はい」 if lib.flag #yes => n += 1
    「いいえ」 #no => end
  }
}
";

#[test]
fn names_inside_control_flow_and_module_qualified_names_resolve_to_declarations() {
    let a = analyze(&[("lib", LIB), ("main", USE)]);
    let count = |kind, name| a.references_to(find(&a, kind, name)).count();
    assert_eq!(
        count(SymbolKind::Var, "flag"),
        3,
        "if の条件・else の代入・選択肢の条件"
    );
    assert_eq!(
        count(SymbolKind::Var, "n"),
        4,
        "while の条件と本体・if の本体・選択肢"
    );
    assert_eq!(count(SymbolKind::Label, "greet"), 1, "式の中の call");
    assert_eq!(
        count(SymbolKind::Struct, "Item"),
        2,
        "型の注釈と struct のリテラル"
    );
    assert_eq!(count(SymbolKind::Identity, "門番"), 1);
    assert_eq!(count(SymbolKind::Local, "it"), 1, "for の変数");
    assert_eq!(count(SymbolKind::Local, "got"), 1);
    assert_eq!(count(SymbolKind::Local, "level"), 1, "パターンの変数");
    assert_eq!(
        count(SymbolKind::Variant, "Calm"),
        1,
        "モジュール名つきの 型.バリアント"
    );
    assert_eq!(count(SymbolKind::Enum, "Mood"), 1);
}

#[test]
fn variable_fields_methods_and_assignments_refer_to_variable_not_module_alias() {
    let src = "\
identity 案内人 = \"案内人\"
struct P { name: str, xs: list<int> }
save var g: P = P { name: \"g\", xs: [] }
label start {
  let p = P { name: \"a\", xs: [] }
  ${p.name}
  let n = p.xs.len()
  p.xs.push(1)
  g.name = 案内人.name
}
";
    let a = analyze(&[("main", src)]);
    let spans = |kind, name| {
        let id = find(&a, kind, name);
        let mut v: Vec<(u32, u32)> = a
            .references_to(id)
            .map(|r| (r.span.line, r.span.col))
            .collect();
        v.sort();
        v
    };
    assert_eq!(spans(SymbolKind::Local, "p"), [(6, 5), (7, 11), (8, 3)]);
    assert_eq!(spans(SymbolKind::Var, "g"), [(9, 3)]);
    assert_eq!(spans(SymbolKind::Identity, "案内人"), [(9, 12)]);
}

#[test]
fn lambda_params_and_fn_names_as_values_become_references() {
    let src = "fn double(n: int) -> int {\n  return n * 2\n}\nlabel start {\n  let k = 1\n  let xs = [1].map(|x| x + k).map(double)\n  ${xs}\n}\n";
    let a = analyze(&[("main", src)]);
    let x = find(&a, SymbolKind::Local, "x");
    assert_eq!(a.references_to(x).count(), 1);
    let k = find(&a, SymbolKind::Local, "k");
    assert_eq!(a.references_to(k).count(), 1);
    let double = find(&a, SymbolKind::Fn, "double");
    assert_eq!(a.references_to(double).count(), 1);
}

#[test]
fn variables_of_two_variable_for_and_each_are_symbols_with_resolved_references() {
    let src = "label start {\n  let m = map { \"a\": 1 }\n  for k, v in m {\n    ${k}${v}${v}\n  }\n  [1].each { i, x =>\n    ${i}\n  }\n}\n";
    let a = analyze(&[("main", src)]);
    let count = |name| a.references_to(find(&a, SymbolKind::Local, name)).count();
    assert_eq!(
        (count("k"), count("v"), count("i"), count("x")),
        (1, 2, 1, 0)
    );
}
