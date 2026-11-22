//! Syntax and type checking

use super::*;

#[test]
fn fn_can_be_declared_and_called_in_expressions() {
    let f = "fn double(x: int) -> int {\n  return x * 2\n}\n";
    // `let`, `${}`, conditions, inside arguments, the subject of `match`, combined with operators
    assert_eq!(one(f, "  let y = double(21)\n  ${y}"), "42");
    assert_eq!(one(f, "  ${double(4)}"), "8");
    assert_eq!(one(f, "  if double(2) == 4 {\n    はい\n  }"), "はい");
    assert_eq!(one(f, "  ${double(double(3))}"), "12");
    assert_eq!(one(f, "  ${1 + double(2) * double(3)}"), "25");
    assert_eq!(
        one(f, "  match double(2) {\n    4 => 四\n    _ => ほか\n  }"),
        "四"
    );
    assert_eq!(
        one(f, "  let xs = [double(1), double(2)]\n  ${xs}"),
        "[2, 4]"
    );
    assert_eq!(one(f, "  while double(1) < 0 {\n  }\n  終"), "終");
}

#[test]
fn fn_without_return_value_can_be_called_but_receiving_value_is_error() {
    let f = "fn noop(x: int) {\n  let y = x\n  y += 1\n}\n";
    // A `fn` without a return value can be called in an `@` statement (it has no side effects, so nothing happens)
    assert_eq!(one(f, "  @noop(1)\n  済"), "済");
    // The value cannot be received
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  let a = noop(1)\n}}\n")),
        ["call_without_value"]
    );
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  let a = @noop(1)\n}}\n")),
        ["call_without_value"]
    );
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  ${{1 + noop(1)}}\n}}\n")),
        ["call_without_value"]
    );
}

#[test]
fn fn_with_return_value_can_be_called_as_statement_discarding_value() {
    let f = "fn double(x: int) -> int {\n  return x * 2\n}\n";
    assert_eq!(one(f, "  @double(3)\n  済"), "済");
    assert_eq!(one(f, "  let z = @double(4)\n  ${z}"), "8");
    assert_eq!(one(f, "  let z = 1\n  z = @double(5)\n  ${z}"), "10");
    // Stable IDs are only for stop lines
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  @double(3) #a\n}}\n")),
        ["invalid_stable_id"]
    );
}

#[test]
fn fn_declaration_syntax_errors() {
    // The `( )` of parameters is required
    assert_eq!(
        compile_codes("fn f {\n}\nlabel start {\n}\n"),
        ["unexpected_token"]
    );
    // Type annotations of parameters are required
    assert_eq!(
        compile_codes("fn f(x) {\n}\nlabel start {\n}\n"),
        ["unexpected_token"]
    );
    // An empty `fn`, and one without parameters but with a return value, can be written
    assert!(
        compile_codes("fn f() {\n}\nfn g() -> int {\n  return 1\n}\nlabel start {\n}\n").is_empty()
    );
    // An unclosed `fn`
    assert_eq!(
        compile_codes("fn f() {\n  let _a = 1\n"),
        ["unterminated_block"]
    );
    // A `fn` cannot be declared inside a label
    assert_eq!(
        compile_codes("label start {\n  fn f() {\n  }\n}\n"),
        ["declaration_not_allowed"]
    );
    // `import` is written before `fn`
    assert_eq!(
        compile_codes("fn f() {\n}\nimport x\nlabel start {\n}\n"),
        ["import_not_first", "unknown_module"]
    );
}

#[test]
fn argument_count_and_types_are_checked() {
    let f = "fn add(a: int, b: int) -> int {\n  return a + b\n}\n";
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  let x = add(1)\n}}\n")),
        ["arg_count"]
    );
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  let x = add(1, 2, 3)\n}}\n")),
        ["arg_count"]
    );
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  let x = add(1, \"a\")\n}}\n")),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes(&format!(
            "{f}label start {{\n  let x: str = add(1, 2)\n}}\n"
        )),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes(&format!("{f}label start {{\n  @add(1)\n}}\n")),
        ["arg_count"]
    );
}

#[test]
fn return_type_and_return_checks() {
    assert_eq!(
        compile_codes("fn f() -> int {\n  return \"a\"\n}\nlabel start {\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("fn f() -> int {\n  return\n}\nlabel start {\n}\n"),
        ["missing_return_value"]
    );
    assert_eq!(
        compile_codes("fn f() {\n  return 1\n}\nlabel start {\n}\n"),
        ["unexpected_return_value"]
    );
    // A `return` without a value can be written in a `fn` without a return value
    assert!(
        compile_codes("fn f(x: int) {\n  if x > 0 {\n    return\n  }\n}\nlabel start {\n}\n")
            .is_empty()
    );
}

#[test]
fn missing_return_unless_every_path_returns_value() {
    assert_eq!(
        compile_codes(
            "fn f(x: int) -> int {\n  if x > 0 {\n    return 1\n  }\n}\nlabel start {\n}\n"
        ),
        ["missing_return"]
    );
    // Returning through `else` is fine
    assert!(compile_codes(
        "fn f(x: int) -> int {\n  if x > 0 {\n    return 1\n  } else {\n    return 2\n  }\n}\nlabel start {\n}\n"
    )
    .is_empty());
    // `while true` without breaking out does not reach the end
    assert!(
        compile_codes(
            "fn f(x: int) -> int {\n  while true {\n    return x\n  }\n}\nlabel start {\n}\n"
        )
        .is_empty()
    );
    // `while` with `break` may reach the end
    assert_eq!(
        compile_codes(
            "fn f(x: int) -> int {\n  while true {\n    break\n  }\n}\nlabel start {\n}\n"
        ),
        ["missing_return"]
    );
    // Fine if all arms of `match` return
    assert!(compile_codes(
        "fn f(x: bool) -> int {\n  match x {\n    true => return 1\n    false => return 2\n  }\n}\nlabel start {\n}\n"
    )
    .is_empty());
    // `for` may run 0 times, so it cannot return
    assert_eq!(
        compile_codes(
            "fn f(xs: list<int>) -> int {\n  for x in xs {\n    return x\n  }\n}\nlabel start {\n}\n"
        ),
        ["missing_return"]
    );
}

#[test]
fn unreachable_statements_are_not_even_warned() {
    let src = "fn f() -> int {\n  return 1\n  let _a = 2\n}\nlabel start {\n  ${f()}\n}\n";
    assert!(compile_codes(src).is_empty());
    assert!(warning_codes(src).is_empty());
}

#[test]
fn statements_not_allowed_in_fn_are_errors() {
    for (body, what) in [
        ("  素の行です", "plain"),
        ("  「引用」", "quote"),
        ("  jump start", "jump"),
        ("  call start()", "call"),
        ("  end", "end"),
        ("  choice {\n    「a」 => {\n    }\n  }", "choice"),
        ("  @ui.pause()", "@ui.pause"),
        ("  @ui.say(\"x\")", "@ui.say"),
    ] {
        let src = format!("fn f() {{\n{body}\n}}\nlabel start {{\n}}\n");
        let reg = registry();
        let d = compile(&src, "main", &reg).unwrap_err();
        assert_eq!(d.len(), 1, "{body}: {d:?}");
        assert_eq!(d[0].code.as_str(), "not_allowed_in_fn", "{body}");
        assert_eq!(d[0].arg_str("what"), Some(what), "{body}");
        assert_eq!((d[0].line, d[0].col), (2, 3), "{body}");
    }
    // Same in the form that receives the return value
    assert_eq!(
        compile_codes("fn f() {\n  let a = @ui.pause()\n}\nlabel start {\n}\n"),
        ["not_allowed_in_fn"]
    );
    assert_eq!(
        compile_codes(
            "label g() -> int {\n  return 1\n}\nfn f() {\n  let a = call g()\n}\nlabel start {\n}\n"
        ),
        ["not_allowed_in_fn"]
    );
}

#[test]
fn errors_in_fn_bodies_are_reported_with_others_in_position_order() {
    let src = "fn f(x: int) -> int {\n  jump start\n  let a: str = x\n  return a\n}\nlabel start {\n  let b: int = \"s\"\n}\n";
    assert_eq!(
        compile_codes(src),
        [
            "not_allowed_in_fn",
            "type_mismatch",
            "type_mismatch",
            "type_mismatch"
        ]
    );
}

#[test]
fn fn_can_read_but_not_write_module_variables() {
    let pre = "save var score: int = 5\npersistent var best: int = 7\nsave var items: list<int> = [1]\nstruct P { hp: int }\nsave var p: P = P { hp: 3 }\n";
    // Reading is fine
    let ok = format!(
        "{pre}fn f() -> int {{\n  return score + best + items[0] + p.hp\n}}\nlabel start {{\n  ${{f()}}\n}}\n"
    );
    assert_eq!(texts(&ok), ["16", "<end>"]);
    // Assignment, compound assignment, fields, indices and method statements cannot be written
    for stmt in [
        "score = 1",
        "score += 1",
        "best = 1",
        "p.hp = 1",
        "p.hp -= 1",
        "items[0] = 1",
        "items.push(1)",
        "items.clear()",
        "main.score = 1",
    ] {
        let src = format!("{pre}fn f() {{\n  {stmt}\n}}\nlabel start {{\n}}\n");
        let reg = registry();
        let d = compile(&src, "main", &reg).unwrap_err();
        assert_eq!(d.len(), 1, "{stmt}: {d:?}");
        assert_eq!(d[0].code.as_str(), "assign_in_fn", "{stmt}");
    }
    // The diagnostic argument is the name of the variable written
    let reg = registry();
    let d = compile(
        &format!("{pre}fn f() {{\n  score = 1\n  main.best = 2\n}}\nlabel start {{\n}}\n"),
        "main",
        &reg,
    )
    .unwrap_err();
    assert_eq!(d[0].arg_str("name"), Some("score"));
    assert_eq!(d[1].arg_str("name"), Some("main.best"));
    // Writing to captured local variables is fine
    let ok = format!(
        "{pre}fn g(x: int, xs: list<int>) -> int {{\n  let y = x\n  y += 1\n  x = 9\n  xs.push(y)\n  xs[0] = x\n  return xs[0] + xs[1]\n}}\nlabel start {{\n  ${{g(1, [0])}}\n}}\n"
    );
    assert_eq!(texts(&ok), ["11", "<end>"]);
}

#[test]
fn caller_locals_are_not_visible_from_fn() {
    assert_eq!(
        compile_codes("fn f() -> int {\n  return a\n}\nlabel start {\n  let a = 1\n  ${f()}\n}\n"),
        ["unknown_name"]
    );
}

#[test]
fn builtins_and_queries_can_be_called_in_fn() {
    let f = "fn roll() -> int {\n  return random(1, 6)\n}\nfn s() -> bool {\n  return seen(\"start#a\") || visited(\"start\")\n}\nfn r(n: int) -> int {\n  return to_int(to_float(n) * 1.5)\n}\nfn total(n: int) -> int {\n  let t = 0\n  for i in range(0, n) {\n    t += i\n  }\n  return t\n}\n";
    let t = texts(&format!(
        "{f}label start {{\n  一 #a\n  ${{roll() >= 1 && roll() <= 6}} ${{s()}} ${{r(3)}} ${{total(5)}}\n}}\n"
    ));
    assert_eq!(t, ["一", "true true 4 10", "<end>"]);
}

#[test]
fn random_inside_fn_returns_fixed_sequence() {
    let f = "fn roll() -> int {\n  return random(1, 1000)\n}\n";
    let a = texts_with(
        &format!("{f}label start {{\n  ${{roll()}} ${{roll()}}\n}}\n"),
        VmConfig::default().seed(7),
    );
    let b = texts_with(
        &format!("{f}label start {{\n  ${{random(1, 1000)}} ${{random(1, 1000)}}\n}}\n"),
        VmConfig::default().seed(7),
    );
    // Going through a `fn` does not change the PRNG sequence
    assert_eq!(a, b);
}

#[test]
fn name_collisions() {
    // The same name as a variable, label, identity, type or another fn
    for src in [
        "save var f: int = 0\nfn f() {\n}\nlabel start {\n}\n",
        "fn f() {\n}\nsave var f: int = 0\nlabel start {\n}\n",
        "label f {\n}\nfn f() {\n}\nlabel start {\n}\n",
        "fn f() {\n}\nlabel f {\n}\nlabel start {\n}\n",
        "fn f() {\n}\nfn f() {\n}\nlabel start {\n}\n",
        "struct f { a: int }\nfn f() {\n}\nlabel start {\n}\n",
        "identity f = \"x\"\nfn f() {\n}\nlabel start {\n}\n",
    ] {
        let codes: Vec<_> = compile_codes(src)
            .into_iter()
            .filter(|c| *c != "label_falls_through")
            .collect();
        assert_eq!(codes, ["duplicate_name"], "{src}");
    }
    // The same name as a command (a stop command registered directly)
    let mut reg = registry();
    reg.stop::<(), ()>("beep").unwrap();
    let d = compile("fn beep() {\n}\nlabel start {\n}\n", "main", &reg).unwrap_err();
    assert_eq!(d[0].code.as_str(), "duplicate_name");
    // Colliding parameter names
    assert_eq!(
        compile_codes("fn f(a: int, a: int) {\n}\nlabel start {\n}\n"),
        ["duplicate_name"]
    );
}

#[test]
fn fn_with_builtin_name_shadows_with_warning_and_fn_takes_precedence() {
    let src = "fn random(a: int, b: int) -> int {\n  return a + b\n}\nlabel start {\n  ${random(1, 2)}\n}\n";
    assert_eq!(warning_codes(src), ["shadowed_builtin"]);
    assert_eq!(texts(src), ["3", "<end>"]);
}

#[test]
fn undeclared_functions_are_unknown_function_with_close_suggestions() {
    let reg = registry();
    let d = compile(
        "fn double(x: int) -> int {\n  return x\n}\nlabel start {\n  let a = doubel(1)\n  let b = to_inx(1.5)\n  let c = nothing(1)\n  @doubl(2)\n}\n",
        "main",
        &reg,
    )
    .unwrap_err();
    let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes,
        [
            "unknown_function",
            "unknown_function",
            "unknown_function",
            "unknown_command"
        ]
    );
    assert_eq!(
        d[0].arg_list("suggestions"),
        Some(&["double".to_string()][..])
    );
    assert_eq!(
        d[1].arg_list("suggestions"),
        Some(&["to_int".to_string()][..])
    );
    assert_eq!(d[2].arg_list("suggestions"), None);
}

#[test]
fn context_hints_apply_to_parameter_and_return_types() {
    let src = "fn first(xs: list<int>) -> Option<int> {\n  if xs.is_empty() {\n    return None\n  }\n  return Some(xs[0])\n}\nfn none() -> list<str> {\n  return []\n}\nlabel start {\n  let a = first([])\n  let b = first([4, 5])\n  let c = none()\n  ${a} ${b} ${c}\n}\n";
    assert_eq!(texts(src), ["None Some(4) []", "<end>"]);
    // Undecidable `[]` and `None` are as before
    assert_eq!(
        compile_codes("fn f() {\n  let a = []\n}\nlabel start {\n}\n"),
        ["cannot_infer_type"]
    );
}

#[test]
fn local_with_variable_name_does_not_prevent_calling_fn() {
    // Even if there is a local variable named `double`, `double(…)` is a call of the `fn`
    let f = "fn double(x: int) -> int {\n  return x * 2\n}\n";
    assert_eq!(one(f, "  let double = 5\n  ${double(double)}"), "10");
}
