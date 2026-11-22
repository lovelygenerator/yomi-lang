//! Basics

use super::*;

#[test]
fn compile_with_warnings_returns_them_in_position_order() {
    let src = "save var random: int = 1\nlabel start {\n  let seen = 2\n  let to_int = 3\n}\n";
    let (program, warnings) = compile_with_warnings(src, "m", &registry()).unwrap();
    assert_eq!(program.module(), "m");
    let got: Vec<_> = warnings
        .iter()
        .map(|d| (d.line, d.col, d.severity, d.code, d.arg_str("name")))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                1,
                10,
                Severity::Warning,
                DiagCode::ShadowedBuiltin,
                Some("random")
            ),
            (
                3,
                7,
                Severity::Warning,
                DiagCode::ShadowedBuiltin,
                Some("seen")
            ),
            (
                3,
                7,
                Severity::Warning,
                DiagCode::UnusedVariable,
                Some("seen")
            ),
            (
                4,
                7,
                Severity::Warning,
                DiagCode::ShadowedBuiltin,
                Some("to_int")
            ),
            (
                4,
                7,
                Severity::Warning,
                DiagCode::UnusedVariable,
                Some("to_int")
            ),
        ]
    );
}

#[test]
fn compile_drops_warnings_and_returns_same_program() {
    let src = "save var random: int = 1\nlabel start {\n  a\n}\n";
    let reg = registry();
    let a = compile(src, "m", &reg).unwrap();
    let (b, w) = compile_with_warnings(src, "m", &reg).unwrap();
    assert_eq!(a, b);
    assert_eq!(w.len(), 1);
}

#[test]
fn scripts_without_warnings_return_empty_warnings() {
    let (_, w) = compile_with_warnings("label start {\n  a\n}\n", "m", &registry()).unwrap();
    assert!(w.is_empty());
}

#[test]
fn errors_return_errors_and_warnings_together_in_position_order() {
    let src = "save var random: int = 1\nlabel start {\n  x = 1\n}\n";
    let d = compile_with_warnings(src, "m", &registry()).unwrap_err();
    let got: Vec<_> = d.iter().map(|d| (d.severity, d.code)).collect();
    assert_eq!(
        got,
        vec![
            (Severity::Warning, DiagCode::ShadowedBuiltin),
            (Severity::Error, DiagCode::UnknownName),
        ]
    );
    // `compile` returns diagnostics including warnings as they are
    let d2 = compile(src, "m", &registry()).unwrap_err();
    assert_eq!(d, d2);
}

#[test]
fn variable_names_work_even_when_clashing_with_builtins() {
    // The declared one takes priority. The call is of a variable, so it cannot be used yet
    let src =
        "save var to_int: int = 1\nlabel start {\n  let a = to_int + 1\n  let b = to_int(1.5)\n}\n";
    let d = diags(src);
    assert_eq!(
        d.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![DiagCode::ShadowedBuiltin, DiagCode::NotYetSupported]
    );
}

#[test]
fn close_name_candidates_are_same_kind_only() {
    // Even `gld`, close to the variable `gold`, does not make the label `gold2` a candidate for variables
    let src = "save var gold: int = 0\nlabel gld2 {\n}\nlabel start {\n  gld += 1\n  jump gld\n}\n";
    let d = diags(src);
    assert_eq!(d.len(), 2);
    assert_eq!(d[0].code, DiagCode::UnknownName);
    assert_eq!(
        d[0].arg_list("suggestions"),
        Some(&["gold".to_string()][..])
    );
    assert_eq!(d[1].code, DiagCode::UnknownLabel);
    assert_eq!(
        d[1].arg_list("suggestions"),
        Some(&["gld2".to_string()][..])
    );
}

#[test]
fn close_name_candidates_are_at_most_3_by_distance() {
    let src = "save var abcdef: int = 0
save var abcdeg: int = 0
save var abcdxx: int = 0
save var abcxxx: int = 0
save var abcdefg: int = 0
label start {
  abcdef2 = 1
}
";
    let d = diags(src);
    assert_eq!(d.len(), 1);
    assert_eq!(
        d[0].arg_list("suggestions"),
        // abcdef and abcdefg have distance 1. abcdeg and abcdxx have distance 2 (equal distances in name order)
        Some(
            &[
                "abcdef".to_string(),
                "abcdefg".to_string(),
                "abcdeg".to_string()
            ][..]
        )
    );
}

#[test]
fn short_names_are_close_within_distance_1_and_long_within_2() {
    let reg = registry();
    let name = |src: &str| -> Option<Vec<String>> {
        let d = compile(src, "m", &reg).unwrap_err();
        d[0].arg_list("suggestions").map(<[String]>::to_vec)
    };
    // The shorter one is 4 characters or less: distance 1
    assert_eq!(
        name("save var gold: int = 0\nlabel start {\n  gld = 1\n}\n"),
        Some(vec!["gold".to_string()])
    );
    assert_eq!(
        name("save var gold: int = 0\nlabel start {\n  gl = 1\n}\n"),
        None
    );
    // 5 characters or more: distance 2
    assert_eq!(
        name("save var health: int = 0\nlabel start {\n  heatlh = 1\n}\n"),
        Some(vec!["health".to_string()])
    );
    assert_eq!(
        name("save var health: int = 0\nlabel start {\n  hlt = 1\n}\n"),
        None
    );
}

#[test]
fn close_command_names_are_also_suggested() {
    let d = diags("label start {\n  @game.giev(\"a\", 1)\n}\n");
    assert_eq!(d[0].code, DiagCode::UnknownCommand);
    assert_eq!(
        d[0].arg_list("suggestions"),
        Some(&["game.give".to_string()][..])
    );
}

#[test]
fn using_names_of_failed_declarations_is_not_reported_undeclared() {
    let src = "save var a: int = )
label start {
  let x = 1 +
  let y: int =
  x = x + 1
  y += a
  if x == 1 {
    jump broken
  }
  jump later
}
label broken
label later( {
}
";
    // Only parse errors (no cascading of names)
    let all = diags(src);
    assert!(
        all.iter().all(|d| matches!(
            d.code,
            DiagCode::UnexpectedToken | DiagCode::UnexpectedEol | DiagCode::NotYetSupported
        )),
        "{all:?}"
    );
}

#[test]
fn names_of_declarations_with_invalid_types_are_not_called_undeclared() {
    let src = "save var a: foo = 1\nlabel start {\n  a = 2\n  ${a}\n}\n";
    assert_eq!(errors(src), vec![DiagCode::UnknownType]);
}

#[test]
fn no_type_error_cascade_after_expression_with_undecidable_type() {
    // The type of `x` is not decided (`nothing` is undeclared). Later uses of `x` do not produce type errors
    let src = "label start {\n  let x = nothing\n  let y = x + 1\n  let z = x == \"a\"\n  if x {\n  }\n  x += 1\n}\n";
    assert_eq!(errors(src), vec![DiagCode::UnknownName]);
}

#[test]
fn same_named_variable_and_label_reports_the_later_one() {
    let d = diags("label a {\n}\nsave var a: int = 0\nsave var b: int = 0\nlabel b {\n}\n");
    let got: Vec<_> = d.iter().map(|d| (d.line, d.col, d.code)).collect();
    assert_eq!(
        got,
        vec![
            (3, 10, DiagCode::DuplicateName),
            (5, 7, DiagCode::DuplicateName)
        ]
    );
}

#[test]
fn duplicate_let_reported_at_name_column() {
    let d = diags("save var a: int = 1\nlabel start {\n  let   a = 2\n}\n");
    assert_eq!((d[0].line, d[0].col), (3, 9));
}

#[test]
fn break_and_continue_outside_loop_are_errors() {
    assert_eq!(
        errors("label a {\n  break\n  continue\n  while true {\n    break\n  }\n}\n"),
        vec![DiagCode::OutsideLoop, DiagCode::OutsideLoop]
    );
    let d = diags("label a {\n  if true {\n    continue\n  }\n}\n");
    assert_eq!(d[0].arg_str("what"), Some("continue"));
}

#[test]
fn while_condition_must_be_bool() {
    let d = diags("label a {\n  while 1.5 {\n  }\n}\n");
    assert_eq!(d[0].code, DiagCode::ConditionNotBool);
    assert_eq!(d[0].arg_str("found"), Some("float"));
}

#[test]
fn let_inside_while_is_not_visible_outside() {
    let src = "label a {\n  while true {\n    let x = 1\n    break\n  }\n  x = 2\n}\n";
    assert_eq!(errors(src), vec![DiagCode::UnknownName]);
}

#[test]
fn operations_unusable_on_type_are_invalid_operation() {
    let d = diags(
        "label a {\n  let a = true + 1\n  let b = -\"x\"\n  let c = \"a\" <= \"b\"\n  let d = 1.0 % 2.0\n}\n",
    );
    let got: Vec<_> = d
        .iter()
        .map(|d| (d.code, d.arg_str("op"), d.arg_str("type")))
        .collect();
    assert_eq!(
        got,
        vec![
            (DiagCode::InvalidOperation, Some("+"), Some("bool")),
            (DiagCode::InvalidOperation, Some("-"), Some("str")),
            (DiagCode::InvalidOperation, Some("<="), Some("str")),
            (DiagCode::InvalidOperation, Some("%"), Some("float")),
        ]
    );
    // When only the right-hand side has an unusable type (the left is a number), it is a type mismatch
    assert_eq!(
        errors("label a {\n  let a = 1 + \"x\"\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
}

#[test]
fn comparison_for_same_numeric_types_and_equality_for_same_types() {
    assert!(errors("label a {\n  let a = 1.5 < 2.5\n  let b = 1 <= 2\n  let c = \"a\" == \"b\"\n  let d = true != false\n  let e = 1.5 == 2.5\n}\n").is_empty());
    assert_eq!(
        errors("label a {\n  let a = 1 < 2.5\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
}

#[test]
fn compound_assignment_type_errors_do_not_repeat_same_type() {
    let d = diags("save var s: str = \"a\"\nlabel a {\n  s += \"b\"\n}\n");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, DiagCode::InvalidOperation);
    assert_eq!(d[0].arg_str("op"), Some("+="));
    assert_eq!(d[0].arg_str("type"), Some("str"));
    assert_eq!((d[0].line, d[0].col), (3, 3));
}

#[test]
fn checks_builtin_calls() {
    assert!(errors("label a {\n  let a = to_int(1.5)\n  let b = to_float(1)\n  let c = to_int(to_float(2) * 1.5)\n}\n").is_empty());
    assert_eq!(
        errors("label a {\n  let a = to_int(1)\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    assert_eq!(
        errors("label a {\n  let a = to_float(1, 2)\n}\n"),
        vec![DiagCode::ArgCount]
    );
    // The result type is decided (even with argument errors), so there is no cascading
    assert_eq!(
        errors("label a {\n  let a = to_int(1)\n  let b: int = a\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    // `random` takes two `int`s and returns an `int`
    assert!(
        errors("label a {\n  let a = random(1, 6)\n  let b: int = random(a, a + 1)\n}\n")
            .is_empty()
    );
    assert_eq!(
        errors("label a {\n  let a = random(1)\n}\n"),
        vec![DiagCode::ArgCount]
    );
    assert_eq!(
        errors("label a {\n  let a = random(1, 2.5)\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    assert_eq!(
        errors("label a {\n  let a = random(1, 2)\n  let b: str = a\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    // For an unknown function, close names among built-in functions are candidates
    let d = diags("label a {\n  let a = to_inx(1.5)\n}\n");
    assert_eq!(d[0].code, DiagCode::UnknownFunction);
    assert_eq!(
        d[0].arg_list("suggestions"),
        Some(&["to_int".to_string()][..])
    );
}

#[test]
fn float_initial_values_can_be_constants() {
    assert!(
        errors("save var a: float = 1.5\nsave var b: float = -2.25\nlabel s {\n}\n").is_empty()
    );
    assert_eq!(
        errors("save var a: float = 1\nlabel s {\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    assert_eq!(
        errors("save var a: float = 1.5 + 1.0\nlabel s {\n}\n"),
        vec![DiagCode::NotConstant]
    );
}

#[test]
fn block_depth_limit_applies_to_while() {
    let src = format!(
        "label a {{\n{}{}}}\n",
        "if true {\n".repeat(30) + &"while true {\n".repeat(40),
        "}\n".repeat(70)
    );
    assert_eq!(errors(&src), vec![DiagCode::BlockTooDeep]);
}

#[test]
fn duplicate_stable_ids_are_checked_per_label() {
    let src = "label a {\n  あ #x\n  い #x\n}\nlabel b {\n  う #x\n}\n";
    let d = diags(src);
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].line, d[0].code), (3, DiagCode::DuplicateStableId));
    // Even with a large number of IDs, it does not take time (HashSet)
    let mut big = String::from("label a {\n");
    for i in 0..50_000 {
        big.push_str(&format!("  あ #i{i}\n"));
    }
    big.push_str("}\n");
    let t = std::time::Instant::now();
    assert!(errors(&big).is_empty());
    assert!(t.elapsed() < std::time::Duration::from_secs(10));
}

pub(super) fn modules(
    sources: &[(&str, &str)],
) -> Result<(Program, Vec<Diagnostic>), Vec<Diagnostic>> {
    compile_modules_with_warnings(sources, &registry())
}

#[test]
fn combines_multiple_modules_into_one_program() {
    let (p, w) = modules(&[
        ("main", "import lib\nlabel start {\n  call lib.f(1)\n}\n"),
        (
            "lib",
            "save var v: int = 0\nlabel f(n: int) {\n  lib.v += n\n}\n",
        ),
    ])
    .unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(p.module_names().collect::<Vec<_>>(), vec!["main", "lib"]);
    assert_eq!(p.labels_of("lib"), vec!["f"]);
    assert_eq!(p.labels_of("main"), vec!["start"]);
    assert!(p.labels_of("nothing").is_empty());
}

#[test]
fn cyclic_imports_can_be_written() {
    let r = modules(&[
        ("a", "import b\nlabel x {\n  jump b.y\n}\n"),
        ("b", "import a\nlabel y {\n  jump a.x\n}\n"),
    ]);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn module_name_errors_are_at_line_1_column_1_of_that_module() {
    let d = modules(&[
        ("ok", "label start {\n}\n"),
        ("ok", "label start {\n}\n"),
        ("1x", ""),
        ("has space", ""),
        ("", ""),
        ("let", ""),
    ])
    .unwrap_err();
    let got: Vec<_> = d
        .iter()
        .map(|d| (d.module.as_str(), d.line, d.col, d.code))
        .collect();
    assert_eq!(
        got,
        vec![
            ("ok", 1, 1, DiagCode::DuplicateModule),
            ("1x", 1, 1, DiagCode::InvalidModuleName),
            ("has space", 1, 1, DiagCode::InvalidModuleName),
            ("", 1, 1, DiagCode::InvalidModuleName),
            ("let", 1, 1, DiagCode::InvalidModuleName),
        ]
    );
}

#[test]
fn diagnostic_module_name_is_where_it_occurred() {
    let d = modules(&[
        ("a", "label start {\n  jump nowhere\n}\n"),
        ("b", "label start {\n  jump nowhere\n}\n"),
    ])
    .unwrap_err();
    assert_eq!(
        d.iter().map(|d| d.module.as_str()).collect::<Vec<_>>(),
        vec!["a", "b"]
    );
}

#[test]
fn unimported_and_nonexistent_modules_are_different_errors() {
    let d = modules(&[
        (
            "main",
            "label start {\n  jump other.x\n  jump ghost.x\n  let v = other.v\n}\n",
        ),
        ("other", "save var v: int = 0\nlabel x {\n  end\n}\n"),
    ])
    .unwrap_err();
    assert_eq!(
        d.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![
            DiagCode::ModuleNotImported,
            DiagCode::UnknownModule,
            DiagCode::ModuleNotImported
        ]
    );
    assert_eq!(d[0].arg_str("name"), Some("other"));
    assert_eq!(d[1].arg_str("name"), Some("ghost"));
}

#[test]
fn importing_unknown_name_gives_diagnostic_with_suggestions_without_repeating_at_uses() {
    let d = modules(&[
        (
            "main",
            "import chaptr\nlabel start {\n  jump chaptr.x\n  let a = chaptr.v\n}\n",
        ),
        ("chapter", "label x {\n  end\n}\n"),
    ])
    .unwrap_err();
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, DiagCode::UnknownModule);
    assert_eq!(
        d[0].arg_list("suggestions"),
        Some(&["chapter".to_string()][..])
    );
}

#[test]
fn own_module_name_can_be_written_without_import() {
    let r = modules(&[(
        "main",
        "save var n: int = 0\nlabel start {\n  main.n = 1\n  jump main.start2\n}\nlabel start2 {\n}\n",
    )]);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn hierarchical_modules_can_be_written_by_last_word_without_import() {
    let (p, w) = modules(&[(
        "story::ch1",
        "save var n: int = 0\nlabel start {\n  ch1.n = 1\n  jump ch1.next\n}\nlabel next {\n}\n",
    )])
    .unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(p.module_names().collect::<Vec<_>>(), vec!["story::ch1"]);
    assert_eq!(p.labels_of("story::ch1"), vec!["start", "next"]);
}

#[test]
fn hierarchical_module_name_parts_must_be_identifiers() {
    for name in ["story::", "::ch1", "a::b-c", "a::label", "a:::b"] {
        let d = modules(&[(name, "label start {\n}\n")]).unwrap_err();
        assert_eq!(d[0].code, DiagCode::InvalidModuleName, "{name}: {d:?}");
    }
    let r = modules(&[("日本語::章1", "label start {\n}\n")]);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn using_imported_module_name_as_variable_label_or_local_is_error() {
    let d = modules(&[
        (
            "main",
            "import lib\nsave var lib: int = 0\nlabel start(lib: int) {\n  let lib = 1\n  return\n}\n",
        ),
        ("lib", "label start {\n}\n"),
    ])
    .unwrap_err();
    assert_eq!(
        d.iter().map(|d| d.code).collect::<Vec<_>>(),
        vec![
            DiagCode::DuplicateName,
            DiagCode::DuplicateName,
            DiagCode::DuplicateName
        ]
    );
}

#[test]
fn same_named_variables_and_labels_in_other_modules_do_not_clash() {
    let r = modules(&[
        ("a", "save var x: int = 1\nlabel x2 {\n}\n"),
        ("b", "save var x: int = 2\nlabel x2 {\n}\n"),
    ]);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn jump_only_to_labels_without_params_or_return_value() {
    let d = diags(
        "label a(n: int) {\n  return\n}\nlabel b() -> int {\n  return 1\n}\nlabel c() {\n}\nlabel start {\n  jump a\n  jump b\n  jump c\n}\n",
    );
    assert_eq!(
        d.iter().map(|d| (d.code, d.line)).collect::<Vec<_>>(),
        vec![
            (DiagCode::JumpToCallableLabel, 10),
            (DiagCode::JumpToCallableLabel, 11)
        ]
    );
}

#[test]
fn label_with_return_type_reaching_end_without_return_is_error() {
    assert_eq!(
        errors("label f() -> int {\n  あ\n}\n"),
        vec![DiagCode::MissingReturn]
    );
    assert!(errors("label f() -> int {\n  return 1\n}\n").is_empty());
    // Returns conditionally. An if without else may reach the end
    assert_eq!(
        errors("label f(x: bool) -> int {\n  if x {\n    return 1\n  }\n}\n"),
        vec![DiagCode::MissingReturn]
    );
    assert!(
        errors(
            "label f(x: bool) -> int {\n  if x {\n    return 1\n  } else {\n    return 2\n  }\n}\n"
        )
        .is_empty()
    );
    // A while true that cannot be exited
    assert!(errors("label f() -> int {\n  while true {\n    return 1\n  }\n}\n").is_empty());
    assert_eq!(
        errors("label f() -> int {\n  while true {\n    break\n  }\n}\n"),
        vec![DiagCode::MissingReturn]
    );
}

#[test]
fn label_never_called_or_jumped_to_without_ending_warns() {
    let w = |src: &str| {
        compile_with_warnings(src, "m", &registry())
            .unwrap()
            .1
            .iter()
            .map(|d| (d.code, d.arg_str("name").map(String::from)))
            .collect::<Vec<_>>()
    };
    let hit = |name: &str| vec![(DiagCode::LabelFallsThrough, Some(name.to_string()))];
    assert_eq!(w("label a(n: int) {\n  あ\n}\n"), hit("a"));
    // Labels without parameters or a return value are also covered
    assert_eq!(w("label a {\n  あ\n}\n"), hit("a"));
    // The default start label `start` is excluded
    assert!(w("label start {\n  あ\n}\n").is_empty());
    // Not emitted if it is `call`ed or `jump`ed to
    assert!(w("label start {\n  call a(1)\n  jump b\n}\nlabel a(n: int) {\n  あ\n}\nlabel b {\n  end\n}\n").is_empty());
    // Not emitted if it ends with return, end or jump
    assert!(w("label a(n: int) {\n  return\n}\n").is_empty());
    assert!(w("label a(n: int) {\n  end\n}\n").is_empty());
    assert!(w("label a(n: int) {\n  jump a2\n}\nlabel a2 {\n  end\n}\n").is_empty());
    // `call`s and `jump`s from other modules also count
    let (_, w2) = modules(&[
        (
            "m",
            "import n\nlabel start {\n  call n.a(1)\n  jump n.b\n}\n",
        ),
        ("n", "label a(x: int) {\n  あ\n}\nlabel b {\n  end\n}\n"),
    ])
    .unwrap();
    assert!(w2.is_empty(), "{w2:?}");
}

#[test]
fn start_labels_can_be_excluded_from_warnings_by_config() {
    let reg = registry();
    let names = |src: &str, entries: &[&str]| {
        let mut o = CompileOptions::default();
        for e in entries {
            o = o.entry_label(e);
        }
        compile_modules_with_options(&[("m", src)], &reg, &o)
            .unwrap()
            .1
            .iter()
            .filter_map(|d| d.arg_str("name").map(String::from))
            .collect::<Vec<_>>()
    };
    let src = "label start {\n  あ\n}\nlabel intro {\n  あ\n}\n";
    // Without specifying, only `start` is excluded
    assert_eq!(names(src, &[]), vec!["intro"]);
    // If specified, `start` is also covered again (`module.label`; with one module, just the name is fine)
    assert_eq!(names(src, &["m.intro"]), vec!["start"]);
    assert_eq!(names(src, &["intro"]), vec!["start"]);
    assert!(names(src, &["m.intro", "m.start"]).is_empty());
    // The same name in another module is not excluded
    assert_eq!(names(src, &["x.intro"]), vec!["start", "intro"]);
}

#[test]
fn call_argument_count_and_types_must_match_declaration() {
    let d = diags(
        "label f(a: int, b: str) {\n}\nlabel start {\n  call f(1)\n  call f(1, 2)\n  call f(\"x\", \"y\")\n  call f(1, \"y\")\n}\n",
    );
    assert_eq!(
        d.iter().map(|d| (d.code, d.line)).collect::<Vec<_>>(),
        vec![
            (DiagCode::ArgCount, 4),
            (DiagCode::TypeMismatch, 5),
            (DiagCode::TypeMismatch, 6)
        ]
    );
    assert_eq!(d[0].arg_str("name"), Some("f"));
}
