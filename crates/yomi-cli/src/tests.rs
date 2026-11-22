use lingokit::Localizer;
use yomi_core::{DiagArg, DiagCode, LoadErrorKind, Ty, VmErrorKind};

use super::*;
use crate::diag::{code_message, error_line, load_kind_message, vm_kind_message};

const LANGS: [&str; 2] = ["en", "ja"];

fn s(v: &str) -> DiagArg {
    DiagArg::Str(v.to_string())
}

#[test]
fn translation_keys_are_complete() {
    lingokit::clap::assert_keys::<Cli>(&LOCALES);
}

#[test]
fn every_diagnostic_code_has_text_in_en_and_ja() {
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for code in DiagCode::ALL {
            let id = format!("diag-{}", code.as_str());
            assert!(loc.has_own(&id), "{lang} に {id} がない");
        }
    }
}

/// Arguments per code (matching the doc comments of `crates/yomi-core/src/diag.rs`).
fn sample_args(code: DiagCode) -> Vec<(&'static str, DiagArg)> {
    match code {
        DiagCode::UnexpectedToken | DiagCode::InvalidEscape => vec![("found", s("@@"))],
        DiagCode::InvalidNumber => vec![("text", s("@@"))],
        DiagCode::UnknownName
        | DiagCode::UnknownCommand
        | DiagCode::UnknownLabel
        | DiagCode::UnknownType
        | DiagCode::DuplicateName
        | DiagCode::ShadowedBuiltin
        | DiagCode::UnknownModule
        | DiagCode::ModuleNotImported
        | DiagCode::DuplicateModule
        | DiagCode::InvalidModuleName
        | DiagCode::PackageAlias
        | DiagCode::DuplicateImport
        | DiagCode::UnusedImport
        | DiagCode::UnusedVariable
        | DiagCode::JumpToCallableLabel
        | DiagCode::JumpInReturningLabel
        | DiagCode::CallWithoutValue
        | DiagCode::MissingReturn
        | DiagCode::AssignInFn
        | DiagCode::UnknownFunction
        | DiagCode::LabelFallsThrough => vec![("name", s("@@"))],
        DiagCode::MissingReturnValue => vec![("expected", s("@exp"))],
        DiagCode::InvalidOperation => vec![("op", s("@@")), ("type", s("@ty"))],
        DiagCode::DeclarationNotAllowed
        | DiagCode::OutsideLoop
        | DiagCode::NotAllowedInFn
        | DiagCode::ReplNotAllowed => {
            vec![("what", s("@@"))]
        }
        DiagCode::TypeMismatch => vec![("expected", s("@exp")), ("found", s("@@"))],
        DiagCode::ConditionNotBool => vec![("found", s("@@"))],
        DiagCode::ArgCount => vec![
            ("name", s("@@")),
            ("expected", DiagArg::Int(1)),
            ("found", DiagArg::Int(2)),
        ],
        DiagCode::DuplicateStableId | DiagCode::InvalidStableId => vec![("id", s("@@"))],
        DiagCode::UnboundSugar => vec![("kind", s("@@"))],
        DiagCode::InvalidCommandUse => vec![("name", s("@@")), ("kind", s("@k"))],
        DiagCode::AssignToReadOnly => vec![("name", s("@@"))],
        DiagCode::DeprecatedCommandName => vec![("name", s("@@")), ("new", s("@new"))],
        DiagCode::NotYetSupported => vec![("what", s("@@"))],
        DiagCode::AmbiguousIdentity => vec![
            ("name", s("@@")),
            (
                "candidates",
                DiagArg::List(vec!["a.@c".into(), "b.@c".into()]),
            ),
        ],
        DiagCode::IdentityTypo => vec![
            ("name", s("@@")),
            (
                "suggestions",
                DiagArg::List(vec!["@s1".into(), "@s2".into()]),
            ),
        ],
        DiagCode::NonExhaustiveMatch => {
            vec![("missing", DiagArg::List(vec!["@m1".into(), "@m2".into()]))]
        }
        DiagCode::UnknownField | DiagCode::UnknownVariant | DiagCode::UnknownMethod => {
            vec![("name", s("@@")), ("type", s("@ty"))]
        }
        DiagCode::MissingField => vec![("name", s("@@")), ("type", s("@ty"))],
        DiagCode::UnsupportedElementType => vec![("method", s("@m")), ("type", s("@ty"))],
        DiagCode::InvalidMapKey => vec![("type", s("@ty"))],
        DiagCode::FnNotSaveable => vec![("type", s("@ty")), ("what", s("@w"))],
        DiagCode::DuplicateField
        | DiagCode::RecursiveType
        | DiagCode::StatementMethodInExpression => {
            vec![("name", s("@@"))]
        }
        DiagCode::UnknownArgument
        | DiagCode::DuplicateArgument
        | DiagCode::MissingArgument
        | DiagCode::ArgumentOutOfOrder
        | DiagCode::RequiredAfterDefault => vec![("name", s("@@")), ("arg", s("@a"))],
        DiagCode::PositionalAfterNamed => vec![("name", s("@@"))],
        DiagCode::UnknownPosition => vec![("position", s("@@"))],
        DiagCode::UnknownLabelLiteral => vec![("label", s("@@"))],
        DiagCode::MissingTextId => vec![("kind", s("@k"))],
        DiagCode::TooDeep | DiagCode::BlockTooDeep | DiagCode::TooManyRegisters => {
            vec![("limit", DiagArg::Int(77))]
        }
        DiagCode::SchemaNameOutsideMigrations
        | DiagCode::MigrateKindMismatch
        | DiagCode::DuplicateMigrateTarget => vec![("name", s("@@"))],
        DiagCode::InvalidSchemaFile => vec![("path", s("@@"))],
        DiagCode::SchemaHashMismatch
        | DiagCode::DuplicateMigrate
        | DiagCode::InvalidMigrateVersion
        | DiagCode::InvalidMinSaveVersion
        | DiagCode::MissingFrozenSchema
        | DiagCode::MissingMigrate
        | DiagCode::SchemaChangedSinceFreeze => vec![("version", DiagArg::Int(77))],
        DiagCode::UnknownSchemaName | DiagCode::UnhandledTypeChange => {
            vec![("name", s("@@")), ("version", DiagArg::Int(77))]
        }
        DiagCode::MigrateTypeMismatch => {
            vec![
                ("name", s("@@")),
                ("expected", s("@exp")),
                ("found", s("@f")),
            ]
        }
        DiagCode::MigrateImpureExpr => vec![("what", s("@@"))],
        DiagCode::UnclosedBracket => vec![("open", s("("))],
        _ => Vec::new(),
    }
}

#[test]
fn diagnostic_args_appear_in_message() {
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for &code in DiagCode::ALL {
            let args = sample_args(code);
            let text = code_message(&loc, code, &args);
            assert!(!text.starts_with("diag-"), "{lang} {code:?}: 文面がない");
            assert!(
                !text.contains("{$"),
                "{lang} {code:?}: 引数が残っている: {text}"
            );
            for (name, value) in &args {
                let shown = match value {
                    DiagArg::Str(v) => v.clone(),
                    DiagArg::Int(n) => n.to_string(),
                    DiagArg::List(v) => v.join(", "),
                };
                assert!(
                    text.contains(&shown),
                    "{lang} {code:?}: 引数 {name} ({shown}) が文面にない: {text}"
                );
            }
        }
    }
}

fn vm_kinds() -> Vec<VmErrorKind> {
    vec![
        VmErrorKind::StepLimitExceeded,
        VmErrorKind::Interrupted,
        VmErrorKind::CallDepthExceeded,
        VmErrorKind::ValueTooLarge,
        VmErrorKind::MemoryLimitExceeded,
        VmErrorKind::IndexOutOfRange { index: 9, len: 3 },
        VmErrorKind::KeyNotFound {
            key: "@k".to_string(),
        },
        VmErrorKind::IntegerOverflow,
        VmErrorKind::DivisionByZero,
        VmErrorKind::NonFiniteFloat,
        VmErrorKind::FloatOutOfRange,
        VmErrorKind::NotStopped,
        VmErrorKind::AlreadyStopped,
        VmErrorKind::Ended,
        VmErrorKind::Poisoned,
        VmErrorKind::BadInput {
            expected: Ty::Int,
            found: Ty::Str,
        },
        VmErrorKind::NoChoiceAvailable,
        VmErrorKind::ChoiceOutOfRange { index: 9, count: 3 },
        VmErrorKind::StartLabelNotFound("@@".to_string()),
        VmErrorKind::CommandMissing("@@".to_string()),
        VmErrorKind::InvalidRandomRange { min: 6, max: 1 },
        VmErrorKind::SnapshotMismatch,
        VmErrorKind::PersistentData(LoadErrorKind::Corrupt),
        VmErrorKind::CommandFailed {
            code: "@@".to_string(),
            args: Vec::new(),
        },
        VmErrorKind::UnknownVariable("@@".to_string()),
        VmErrorKind::SliceSerialize("@@".to_string()),
        VmErrorKind::DeclOnlyRegistry,
    ]
}

#[test]
fn all_runtime_errors_have_text_in_en_and_ja() {
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for kind in vm_kinds() {
            let id = format!("vm-{}", kind.code());
            assert!(loc.has_own(&id), "{lang} に {id} がない");
            let text = vm_kind_message(&loc, &kind, (Some("main"), Some("start")), Some(7));
            assert!(!text.contains("{$"), "{lang}: 引数が残っている: {text}");
            assert!(text.contains("main.start") && text.contains('7'), "{text}");
        }
    }
}

#[test]
fn all_load_errors_have_text_in_en_and_ja() {
    let kinds = vec![
        LoadErrorKind::BadMagic,
        LoadErrorKind::UnsupportedVersion(9),
        LoadErrorKind::ChecksumMismatch,
        LoadErrorKind::Corrupt,
        LoadErrorKind::LabelNotFound("@@".to_string()),
        LoadErrorKind::ModuleMismatch("@@".to_string()),
        LoadErrorKind::CommandMissing("@@".to_string()),
        LoadErrorKind::StopArgsFailed,
        LoadErrorKind::PackageSlice("@@".to_string()),
        LoadErrorKind::DeclOnlyRegistry,
    ];
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for kind in &kinds {
            let id = format!("load-{}", kind.code());
            assert!(loc.has_own(&id), "{lang} に {id} がない");
            let text = load_kind_message(&loc, kind);
            assert!(!text.contains("{$"), "{lang}: 引数が残っている: {text}");
        }
    }
}

#[test]
fn runtime_errors_without_location_show_only_text_and_omit_line_with_only_label() {
    let en = Localizer::only(&LOCALES, "en");
    let ended = VmErrorKind::Ended;
    assert_eq!(
        vm_kind_message(&en, &ended, (None, None), None),
        "the script has already ended"
    );
    assert_eq!(
        vm_kind_message(&en, &ended, (None, Some("start")), None),
        "the script has already ended (label start)"
    );
    let ja = Localizer::only(&LOCALES, "ja");
    assert_eq!(
        vm_kind_message(
            &ja,
            &VmErrorKind::DivisionByZero,
            (None, Some("start")),
            Some(4)
        ),
        "0 で割りました（ラベル start、4 行目）"
    );
    assert_eq!(error_line(&en, "a.yomi", "boom"), "a.yomi: error: boom");
}

#[test]
fn runtime_errors_with_known_module_show_module_label() {
    let ja = Localizer::only(&LOCALES, "ja");
    assert_eq!(
        vm_kind_message(
            &ja,
            &VmErrorKind::DivisionByZero,
            (Some("main"), Some("start")),
            Some(4)
        ),
        "0 で割りました（ラベル main.start、4 行目）"
    );
    let en = Localizer::only(&LOCALES, "en");
    assert_eq!(
        vm_kind_message(
            &en,
            &VmErrorKind::DivisionByZero,
            (Some("main"), Some("start")),
            Some(4)
        ),
        "division by zero (label main.start, line 4)"
    );
}

#[test]
fn adds_did_you_mean_when_close_candidates_exist() {
    let args = vec![
        ("name", s("gld")),
        (
            "suggestions",
            DiagArg::List(vec!["gold".into(), "gald".into()]),
        ),
    ];
    let ja = Localizer::only(&LOCALES, "ja");
    assert_eq!(
        code_message(&ja, DiagCode::UnknownName, &args),
        "宣言されていない名前です: 'gld'（もしかして: gold, gald）"
    );
    let en = Localizer::only(&LOCALES, "en");
    assert_eq!(
        code_message(&en, DiagCode::UnknownName, &args),
        "unknown name 'gld' (did you mean: gold, gald?)"
    );
    // Without candidates, nothing is attached
    let none = vec![("name", s("gld"))];
    assert_eq!(
        code_message(&en, DiagCode::UnknownName, &none),
        "unknown name 'gld'"
    );
}

#[test]
fn all_text_table_errors_and_check_results_have_text_in_en_and_ja() {
    use crate::diag::{text_error_message, text_issue_message};
    use yomi_core::{TextTable, compile};
    // Table files for each kind of error
    let files = [
        "k\tbad \\q\n",
        "k\t${x}\n",
        "no tab\n",
        "\tvalue\n",
        "k\ta\nk\tb\n",
        "@nope\n",
    ];
    let errors: Vec<_> = files
        .iter()
        .map(|f| TextTable::from_file(f).expect_err(f))
        .collect();
    let codes: Vec<&str> = errors.iter().map(|e| e.kind.code()).collect();
    assert_eq!(
        codes,
        [
            "invalid_escape",
            "invalid_placeholder",
            "missing_tab",
            "empty_key",
            "duplicate_key",
            "unknown_directive"
        ]
    );
    // For each kind of check result (missing translation, extra translation, number error)
    let program = compile(
        "label start {\n  あ ${1} #a\n  い #b\n}\n",
        "main",
        &session::registry(),
    )
    .expect("コンパイルできる");
    let mut table = TextTable::new("en");
    table.insert("main.start#a", "x ${2}").unwrap();
    table.insert("main.start#zz", "y").unwrap();
    let issues = table.check(&program);
    let codes: Vec<&str> = issues.iter().map(|i| i.kind.code()).collect();
    assert_eq!(codes, ["bad_number", "missing", "unused"]);
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for e in &errors {
            let id = format!("text-error-{}", e.kind.code());
            assert!(loc.has_own(&id), "{lang} に {id} がない");
            let text = text_error_message(&loc, e);
            assert!(
                !text.contains("{$") && !text.starts_with("text-"),
                "{lang}: {text}"
            );
            if let Some(key) = &e.key {
                assert!(
                    text.contains(key.as_str()) || e.kind.code() != "duplicate_key",
                    "{text}"
                );
            }
        }
        for i in &issues {
            let id = format!("text-issue-{}", i.kind.code());
            assert!(loc.has_own(&id), "{lang} に {id} がない");
            let text = text_issue_message(&loc, i);
            assert!(
                !text.contains("{$") && text.contains(i.key.as_str()),
                "{lang}: {text}"
            );
        }
        // A number error includes the number and the count
        let text = text_issue_message(&loc, &issues[0]);
        assert!(
            text.contains("${2}") && text.contains('1'),
            "{lang}: {text}"
        );
    }
}

#[test]
fn all_declaration_file_errors_have_text_in_en_and_ja() {
    use crate::decls::decl_error_message;
    use yomi_core::{DeclError, DeclErrorKind, RegistryError};
    let kinds = vec![
        DeclErrorKind::Syntax { line: 3, col: 9 },
        DeclErrorKind::TooLarge,
        DeclErrorKind::TooDeep,
        DeclErrorKind::NotDecls,
        DeclErrorKind::UnsupportedVersion(7),
        DeclErrorKind::MissingField,
        DeclErrorKind::WrongType,
        DeclErrorKind::InvalidType("@ty".to_string()),
        DeclErrorKind::InvalidValue,
        DeclErrorKind::Registry(RegistryError::DuplicateCommand("@dup".to_string())),
    ];
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for kind in &kinds {
            let id = format!("decl-error-{}", kind.code());
            assert!(loc.has_own(&id), "{lang} に {id} がない");
            let e = DeclError {
                kind: kind.clone(),
                path: "commands[0]".to_string(),
            };
            let text = decl_error_message(&loc, &e);
            assert!(
                !text.contains("{$") && !text.starts_with("decl-"),
                "{lang}: {text}"
            );
            assert!(text.contains("commands[0]"), "{text}");
            match kind {
                DeclErrorKind::Syntax { .. } => assert!(text.contains('3') && text.contains('9')),
                DeclErrorKind::UnsupportedVersion(_) => assert!(text.contains('7')),
                DeclErrorKind::InvalidType(_) => assert!(text.contains("@ty")),
                DeclErrorKind::Registry(_) => assert!(text.contains("@dup")),
                _ => {}
            }
        }
    }
}

#[test]
fn all_code_list_entries_have_text_in_en_and_ja() {
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for c in yomi_core::all_codes() {
            let id = format!("{}{}", c.group.ftl_prefix(), c.code);
            assert!(loc.has_own(&id), "{lang} に {id} がない");
            // The description with arguments filled in is not empty and has no arguments left over
            let line = crate::codes::human_line(&loc, &c);
            assert!(!line.contains("{$"), "{lang} {id}: {line}");
            assert!(
                !line.contains(&format!("\t{id}")),
                "{lang} {id}: 文面がない: {line}"
            );
        }
    }
}

#[test]
fn diagnostic_sample_arg_names_match_code_arg_names() {
    for &code in DiagCode::ALL {
        let args = sample_args(code);
        let names: Vec<&str> = args.iter().map(|(n, _)| *n).collect();
        for n in &names {
            assert!(
                code.arg_names().contains(n),
                "{}: 一覧にない引数 {n}",
                code.as_str()
            );
        }
        for n in code.arg_names() {
            assert!(
                names.contains(n) || *n == "suggestions",
                "{}: 見本にない引数 {n}",
                code.as_str()
            );
        }
    }
}

/// Reads the output of `yomi codes --json` and returns the `codes` array (also checks `format` and `version`).
fn codes_json_items(text: &str) -> Vec<serde_json::Value> {
    let root: serde_json::Value = serde_json::from_str(text).expect("JSON として読める");
    assert_eq!(root["format"], "yomi-codes");
    assert_eq!(root["version"], 1);
    assert!(text.ends_with("}\n"));
    root["codes"].as_array().expect("codes は配列").clone()
}

#[test]
fn code_list_json_has_all_codes_and_is_language_independent() {
    let items = codes_json_items(&crate::codes::json(&yomi_core::all_codes()));
    let codes = yomi_core::all_codes();
    assert_eq!(items.len(), codes.len());
    for (item, c) in items.iter().zip(&codes) {
        assert_eq!(item["group"], c.group.name(), "{}", c.code);
        assert_eq!(item["code"], c.code);
        assert_eq!(item["args"], serde_json::json!(c.args), "{}", c.code);
    }
    // Diagnostic warnings appear as warnings
    let shadowed = items
        .iter()
        .find(|i| i["code"] == "shadowed_builtin")
        .expect("shadowed_builtin がある");
    assert_eq!(shadowed["severity"], "warning");
}

/// Runs a REPL without a script with in-memory input and output (standard output, standard error, exit code).
fn repl_with(interactive: bool, input: &str) -> (String, String, i32) {
    let loc = Localizer::only(&LOCALES, "en");
    let options = crate::repl::ReplOptions {
        interactive,
        seed: Some(1),
        ..Default::default()
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = crate::repl::run_repl(
        &loc,
        None,
        &options,
        &mut std::io::Cursor::new(input.as_bytes().to_vec()),
        &mut out,
        &mut err,
    );
    (
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
        code,
    )
}

#[test]
fn repl_shows_prompts_only_when_stdin_is_terminal() {
    let input = "1 + 2\nif true {\n  let a = 1\n}\n";
    let (out, err, code) = repl_with(true, input);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "yomi> 3\nyomi> ...> ...> yomi> ");
    let (out, _, code) = repl_with(false, input);
    assert_eq!((code, out.as_str()), (0, "3\n"));
}

#[test]
fn repl_cancel_returns_from_continuation_prompt_to_first_prompt() {
    let (out, err, code) = repl_with(true, "if true {\n:cancel\n7\n");
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, "yomi> ...> yomi> 7\nyomi> ");
}

#[test]
fn no_prompt_while_waiting_at_stop_point_in_repl() {
    let (out, _, code) = repl_with(true, "\\あ\n\n1\n");
    assert_eq!(code, 0);
    assert_eq!(out, "yomi> あ\nyomi> 1\nyomi> ");
}

#[test]
fn packages_list_matches_all_error_codes_and_core_codes_with_descriptions() {
    let expected: Vec<&str> = yomi_std::error_codes()
        .iter()
        .chain(yomi_core::core_command_error_codes())
        .map(|c| c.code)
        .collect();
    let listed: Vec<&str> = crate::codes::command_codes()
        .iter()
        .map(|(_, c)| c.code)
        .collect();
    assert_eq!(listed, expected);
    assert_eq!(expected.len(), 30 + 8);
    for lang in LANGS {
        let loc = Localizer::only(&LOCALES, lang);
        for (kind, c) in crate::codes::command_codes() {
            let line = crate::codes::command_human_line(&loc, kind, c);
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 5, "{line}");
            assert_eq!(cols[0], kind.name());
            assert_eq!(cols[1], c.code);
            assert_eq!(cols[3], c.args.join(","));
            assert!(!cols[4].is_empty() && !line.contains("{$"), "{lang} {line}");
        }
    }
}

#[test]
fn packages_json_has_kind_and_differs_from_json_without_packages() {
    let items = codes_json_items(&crate::codes::json_with_commands(&yomi_core::all_codes()));
    let of = |kind: &str| items.iter().filter(|i| i["kind"] == kind).count();
    let seconds = items
        .iter()
        .find(|i| i["code"] == "flow.negative_seconds")
        .expect("flow.negative_seconds がある");
    assert_eq!(
        *seconds,
        serde_json::json!({
            "kind": "package",
            "code": "flow.negative_seconds",
            "severity": "error",
            "args": ["seconds"],
        })
    );
    assert!(
        items
            .iter()
            .any(|i| i["kind"] == "core" && i["code"] == "core.bad_args")
    );
    assert!(
        items
            .iter()
            .any(|i| i["kind"] == "language" && i["group"] == "diag")
    );
    assert_eq!(of("language"), yomi_core::all_codes().len());
    assert_eq!(of("package"), yomi_std::error_codes().len());
    assert_eq!(of("core"), yomi_core::core_command_error_codes().len());
    assert_eq!(items.len(), of("language") + of("package") + of("core"));
    // The form without `--packages` has no `kind`
    let plain = codes_json_items(&crate::codes::json(&yomi_core::all_codes()));
    assert!(plain.iter().all(|i| i.get("kind").is_none()));
}
