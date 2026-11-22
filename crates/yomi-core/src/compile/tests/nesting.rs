//! Nesting depth and the stack

use super::*;

/// The kind of nesting. Makes a script with `n` levels of nested blocks.
pub(super) fn nested_script(kind: &str, n: usize) -> String {
    let mut src = String::from("label start {\n");
    let mut closes: Vec<&str> = Vec::new();
    for i in 0..n {
        let k = match kind {
            "mix" => [
                "if",
                "while",
                "choice_inline",
                "choice_block",
                "else",
                "else_if",
            ][i % 6],
            k => k,
        };
        match k {
            "if" => {
                src.push_str("if true {\n");
                closes.push("}\n");
            }
            "while" => {
                src.push_str("while true {\n");
                closes.push("}\n");
            }
            // Nesting inside `else { }` and `else if { }`
            "else" => {
                src.push_str("if false {\n} else {\n");
                closes.push("}\n");
            }
            "else_if" => {
                src.push_str("if false {\n} else if false {\n} else if true {\n");
                closes.push("}\n");
            }
            // The body is one statement (nesting of `choice` without a block in between)
            "choice_inline" => {
                src.push_str("choice {\n「a」 => ");
                closes.push("}\n");
            }
            // The body is a `{ }` block (two levels: `choice` and a block)
            _ => {
                src.push_str("choice {\n「a」 => {\n");
                closes.push("}\n}\n");
            }
        }
    }
    src.push_str("end\n");
    for c in closes.iter().rev() {
        src.push_str(c);
    }
    src.push_str("}\n");
    src
}

/// Compiles on a 1 MiB stack (to confirm it does not crash even in debug builds).
pub(super) fn compile_on_small_stack(src: String) -> Vec<DiagCode> {
    crate::tests::on_small_stack(move || full_codes(&src))
}

#[test]
fn nesting_up_to_limit_parses_on_1_mib_stack_and_beyond_is_error() {
    for kind in [
        "if",
        "while",
        "else",
        "else_if",
        "choice_inline",
        "choice_block",
        "mix",
    ] {
        for n in [20, 62, 63, 64, 100] {
            let got = compile_on_small_stack(nested_script(kind, n));
            // `choice_block` counts 2 levels per level (`choice` and the body's block)
            let depth = match kind {
                "choice_block" => 2 * n,
                "mix" => (0..n).map(|i| if i % 6 == 3 { 2 } else { 1 }).sum(),
                _ => n,
            };
            if depth < 64 {
                assert!(got.is_empty(), "{kind} {n}: {got:?}");
            } else {
                assert_eq!(got, vec![DiagCode::BlockTooDeep], "{kind} {n}");
            }
        }
    }
}

#[test]
fn compound_value_nesting_errors_beyond_limit_on_1_mib_stack() {
    let n = 3000;
    let scripts: Vec<(&str, String)> = vec![
        (
            "リスト",
            format!(
                "label s {{\n  let _a = {}1{}\n}}\n",
                "[".repeat(n),
                "]".repeat(n)
            ),
        ),
        (
            "some",
            format!(
                "label s {{\n  let _a = {}1{}\n}}\n",
                "Some(".repeat(n),
                ")".repeat(n)
            ),
        ),
        (
            "型",
            format!(
                "save var a: {}int{} = 1\nlabel s {{\n  end\n}}\n",
                "list<".repeat(n),
                ">".repeat(n)
            ),
        ),
        (
            "フィールドの連鎖",
            format!(
                "struct P {{ p: int }}\nlabel s {{\n  let a = 1{}\n}}\n",
                ".p".repeat(n)
            ),
        ),
        (
            "添字の連鎖",
            format!(
                "label s {{\n  let a = [1]\n  let b = a{}\n}}\n",
                "[0]".repeat(n)
            ),
        ),
        (
            "構造体リテラル",
            format!(
                "struct P {{ p: int }}\nlabel s {{\n  let _a = {}1{}\n}}\n",
                "P { p: ".repeat(n),
                " }".repeat(n)
            ),
        ),
        ("match の入れ子", {
            let mut src = String::from("label s {\n");
            for _ in 0..n {
                src.push_str("match 1 {\n_ => {\n");
            }
            src.push_str("end\n");
            for _ in 0..n {
                src.push_str("}\n}\n");
            }
            src.push_str("}\n");
            src
        }),
    ];
    for (name, src) in scripts {
        let got = compile_on_small_stack(src);
        assert!(!got.is_empty(), "{name}: 上限を超えたのにエラーにならない");
    }
}

#[test]
fn compound_values_up_to_nesting_limit_read_on_1_mib_stack() {
    let src = format!(
        "label s {{\n  let _a = {}1{}\n  end\n}}\n",
        "[".repeat(20),
        "]".repeat(20)
    );
    let got = compile_on_small_stack(src);
    assert!(got.is_empty(), "{got:?}");
    let mut src = String::from("label s {\n  let x = 1\n");
    for _ in 0..30 {
        src.push_str("match x {\n_ => {\n");
    }
    src.push_str("end\n");
    for _ in 0..30 {
        src.push_str("}\n}\n");
    }
    src.push_str("end\n}\n");
    let got = compile_on_small_stack(src);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn persistent_var_requires_type_and_constant_initial_value_and_only_at_top() {
    assert!(
        errors(
            "persistent var a: int = 1\npersistent var b: str = \"x\"\nlabel s {\n  a += 1\n}\n"
        )
        .is_empty()
    );
    // The initial value is a constant
    assert_eq!(
        errors("save var n: int = 1\npersistent var a: int = n + 1\nlabel s {\n}\n"),
        vec![DiagCode::NotConstant]
    );
    // Type mismatch
    assert_eq!(
        errors("persistent var a: int = \"x\"\nlabel s {\n}\n"),
        vec![DiagCode::TypeMismatch]
    );
    // Both a type annotation and an initial value are required
    assert_eq!(
        errors("persistent var a = 1\nlabel s {\n}\n"),
        vec![DiagCode::UnexpectedToken]
    );
    assert_eq!(
        errors("persistent var a: int\nlabel s {\n}\n"),
        vec![DiagCode::UnexpectedEol]
    );
    // Cannot be written inside a label
    let d = diags("label s {\n  persistent var a: int = 1\n}\n");
    assert_eq!(d[0].code, DiagCode::DeclarationNotAllowed);
    assert_eq!(d[0].arg_str("what"), Some("persistent"));
}

#[test]
fn name_clash_between_save_var_and_persistent_var_is_error() {
    assert_eq!(
        errors("save var a: int = 1\npersistent var a: int = 2\nlabel s {\n}\n"),
        vec![DiagCode::DuplicateName]
    );
    assert_eq!(
        errors("persistent var a: int = 1\nsave var a: bool = true\nlabel s {\n}\n"),
        vec![DiagCode::DuplicateName]
    );
    assert_eq!(
        errors("persistent var a: int = 1\nlabel a {\n}\n"),
        vec![DiagCode::DuplicateName]
    );
    // A local variable also cannot have the same name as a visible persistent var
    assert_eq!(
        errors("persistent var a: int = 1\nlabel s {\n  let a = 2\n}\n"),
        vec![DiagCode::DuplicateName]
    );
}

#[test]
fn persistent_var_with_builtin_name_also_warns() {
    let d = diags("persistent var random: int = 1\nlabel s {\n}\n");
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].code, DiagCode::ShadowedBuiltin);
}

#[test]
fn persistent_var_can_be_read_and_written_from_other_modules_with_name() {
    let reg = registry();
    let a = "import b\nlabel start {\n  b.vol = b.vol + 1\n}\n";
    let b = "persistent var vol: int = 40\nlabel unused {\n  end\n}\n";
    assert!(crate::compile_modules(&[("a", a), ("b", b)], &reg).is_ok());
    // Assignment of a different type is an error, the same as save var
    let bad = "import b\nlabel start {\n  b.vol = \"x\"\n}\n";
    assert!(crate::compile_modules(&[("a", bad), ("b", b)], &reg).is_err());
}

#[test]
fn local_variable_types_available_without_annotations() {
    let src = "enum Mode { Idle, Run(int) }\n\nlabel start {\n  let a = 1\n  let b: float = 2.0\n  let xs = [\"a\"]\n  for x in xs {\n    jump start\n  }\n  match Mode.Run(2) {\n    Idle => end\n    Run(n) => end\n  }\n}\n";
    let types = crate::infer_local_types(&[("main", src)], &crate::Registry::new());
    let at = |line: u32, col: u32| {
        types
            .iter()
            .find(|t| t.line == line && t.col == col)
            .map(|t| t.ty.clone())
    };
    assert_eq!(at(4, 7).as_deref(), Some("int"));
    assert_eq!(at(5, 7).as_deref(), Some("float"));
    assert_eq!(at(6, 7).as_deref(), Some("list<str>"));
    assert_eq!(at(7, 7).as_deref(), Some("str"));
    assert_eq!(at(12, 9).as_deref(), Some("int"));
}
