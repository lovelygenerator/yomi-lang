//! Tests of text replacement (localization): `TextTable`, `Vm::set_texts`, `refresh_current`, re-rendering history,
//! table checks, `require_text_ids`.

use super::util::{self, *};
use crate::compile::{CompileOptions, compile_modules_with_options};
use crate::diag::DiagCode;
use crate::program::Program;
use crate::registry::Sugar;
use crate::texts::{TextErrorKind, TextIssueKind};
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Quote, Sugar::Choice], |r| {
        r.stop::<(String, i64), ()>("game.give").unwrap();
    })
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

/// The body string of a stop notification (argument 0 for plain lines, argument 1 for quote lines).
fn body(y: &Yield) -> String {
    args_of(y).last().map(Value::to_text).unwrap_or_default()
}

/// Runs to the end and lists the body of each stop point.
fn all_bodies(vm: &mut Vm<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if vm.is_stopped() {
        resume(vm);
    }
    loop {
        match vm.run().unwrap() {
            Yield::End => return out,
            y => {
                out.push(body(&y));
                resume(vm);
            }
        }
    }
}

const SRC: &str = "identity 案内人 = \"案内人\"\nsave var n: int = 7\nlabel start {\n  朝 ${n} #a\n  案内人「やあ ${n}」 #b\n  ID なし\n  choice {\n    「はい ${n}」 #yes => {\n      はい返事 #e1\n    }\n    「いいえ」 #no => {\n      いいえ返事 #e2\n    }\n  }\n  @game.give(\"薬\", n)\n}\n";

fn full_table() -> TextTable {
    table(&[
        ("main.start#a", "Morning ${1}"),
        ("main.start#b", "「Hi ${1}」"),
        ("main.start#yes", "「Yes ${1}」"),
        ("main.start#no", "「No」"),
        ("main.start#e1", "Yes reply"),
    ])
}

// ---------------------------------------------------------------------------
// TextTable
// ---------------------------------------------------------------------------

#[test]
fn text_table_stores_and_looks_up_keys_and_translations() {
    let mut t = TextTable::new("en");
    assert!(t.is_empty());
    assert_eq!(t.language(), "en");
    t.insert("main.start#a", "one").unwrap();
    t.insert("main.start#b", "two").unwrap();
    assert_eq!(t.len(), 2);
    assert_eq!(t.get("main.start#a"), Some("one"));
    assert_eq!(t.get("main.start#zz"), None);
    // The same key is replaced, and the insertion order does not change
    t.insert("main.start#a", "uno").unwrap();
    assert_eq!(t.len(), 2);
    let all: Vec<_> = t.iter().collect();
    assert_eq!(all, vec![("main.start#a", "uno"), ("main.start#b", "two")]);
}

#[test]
fn invalid_escapes_in_translations_are_errors_and_table_unchanged() {
    let mut t = TextTable::new("en");
    t.insert("k", "keep").unwrap();
    let e = t.insert("k", "bad \\n").unwrap_err();
    assert_eq!(
        e.kind,
        TextErrorKind::InvalidEscape {
            found: "\\n".to_string()
        }
    );
    assert_eq!(e.key.as_deref(), Some("k"));
    assert_eq!(e.kind.code(), "invalid_escape");
    assert_eq!(t.get("k"), Some("keep"));
    // A trailing `\`
    let e = t.insert("k2", "end\\").unwrap_err();
    assert_eq!(
        e.kind,
        TextErrorKind::InvalidEscape {
            found: "\\".to_string()
        }
    );
}

#[test]
fn translation_placeholders_other_than_n_are_errors() {
    let mut t = TextTable::new("en");
    for (src, found) in [
        ("${name}", "${name}"),
        ("${0}", "${0}"),
        ("${ 1 }", "${ 1 }"),
        ("${}", "${}"),
        ("${1+1}", "${1+1}"),
        ("${99999999999}", "${99999999999}"),
        ("開いたまま ${1", "${1"),
    ] {
        let e = t.insert("k", src).unwrap_err();
        assert_eq!(
            e.kind,
            TextErrorKind::InvalidPlaceholder {
                found: found.to_string()
            },
            "{src}"
        );
    }
    assert!(t.is_empty());
}

#[test]
fn translation_n_is_stringified_and_escapes_work() {
    let prog = compile_ok("save var n: int = 3\nlabel start {\n  値 ${n} #a\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[(
        "main.start#a",
        "\\${1} ${1} $5 $ \\\\ \\「\\」\\=\\# ${1}${1}",
    )])))
    .unwrap();
    let y = vm.run().unwrap();
    assert_eq!(body(&y), "${1} 3 $5 $ \\ 「」=# 33");
}

// ---------------------------------------------------------------------------
// Running replacement
// ---------------------------------------------------------------------------

#[test]
fn replaces_plain_quote_and_choice_text_with_translations() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(full_table())).unwrap();
    let y = vm.run().unwrap();
    assert_eq!(body(&y), "Morning 7");
    resume(&mut vm);
    let y = vm.run().unwrap();
    // For quote lines, the identity stays as is and the body is replaced including `「」`
    assert_eq!(
        args_of(&y),
        vec![
            Value::Option(Some(Box::new(Value::Identity(std::sync::Arc::new(
                crate::Identity {
                    id: "案内人".to_string(),
                    name: "案内人".to_string()
                }
            ))))),
            Value::Str("「Hi 7」".into())
        ]
    );
    resume(&mut vm);
    // Lines without a translation (no ID) are the original
    assert_eq!(body(&vm.run().unwrap()), "ID なし");
    resume(&mut vm);
    let y = vm.run().unwrap();
    assert_eq!(
        args_of(&y),
        vec![
            Value::List(vec![Value::Str("「Yes 7」".into()), Value::Str("「No」".into())].into()),
            Value::List(
                vec![
                    Value::Option(Some(Box::new(Value::Str("yes".into())))),
                    Value::Option(Some(Box::new(Value::Str("no".into()))))
                ]
                .into()
            ),
        ]
    );
    resume(&mut vm);
    // The line chosen (the table has the ID, but option 0 was chosen)
    assert_eq!(body(&vm.run().unwrap()), "Yes reply");
}

#[test]
fn translations_can_reorder_repeat_and_omit_indices() {
    let prog = compile_ok(
        "save var a: str = \"X\"\nsave var b: int = 2\nlabel start {\n  ${a} と ${b} と ${b + 1} #a\n}\n",
    );
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[("main.start#a", "${3}/${2}/${2}/${1}/${1}")])))
        .unwrap();
    assert_eq!(body(&vm.run().unwrap()), "3/2/2/X/X");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[("main.start#a", "${2} だけ")])))
        .unwrap();
    assert_eq!(body(&vm.run().unwrap()), "2 だけ");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[("main.start#a", "値なし")])))
        .unwrap();
    assert_eq!(body(&vm.run().unwrap()), "値なし");
}

#[test]
fn missing_from_table_uses_original_and_command_args_are_not_targeted() {
    let prog = compile_ok("label start {\n  あ #a\n  い #b\n  @game.give(\"薬\", 1) #c\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    // `#c` is an `@` command, so it is not replaced even if in the table
    vm.set_texts(Some(table(&[("main.start#a", "A"), ("main.start#c", "C")])))
        .unwrap();
    assert_eq!(all_bodies(&mut vm), vec!["A", "い", "1"]);
    assert_eq!(
        vm.history()[2].args,
        vec![Value::Str("薬".into()), Value::Int(1)]
    );
}

#[test]
fn translations_for_keys_not_in_script_can_be_set() {
    let prog = compile_ok("label start {\n  あ #a\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert!(
        vm.set_texts(Some(table(&[("main.nothing#zz", "x")])))
            .is_ok()
    );
    assert_eq!(all_bodies(&mut vm), vec!["あ"]);
}

#[test]
fn tables_with_indices_beyond_original_count_cannot_be_set_and_previous_table_stays() {
    let prog = compile_ok("save var n: int = 1\nlabel start {\n  あ ${n} #a\n  い #b\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[("main.start#a", "ok ${1}")])))
        .unwrap();
    let bad = table(&[
        ("main.start#a", "one ${1} two ${2}"),
        ("main.start#b", "no ${1}"),
    ]);
    let errors = vm.set_texts(Some(bad)).unwrap_err();
    assert_eq!(errors.len(), 2);
    assert_eq!(
        errors[0].kind,
        TextIssueKind::BadNumber {
            number: 2,
            count: 1
        }
    );
    assert_eq!(errors[0].severity, crate::Severity::Error);
    assert_eq!(errors[0].key, "main.start#a");
    assert_eq!(
        errors[1].kind,
        TextIssueKind::BadNumber {
            number: 1,
            count: 0
        }
    );
    // Still the previous table
    assert_eq!(vm.texts().unwrap().get("main.start#a"), Some("ok ${1}"));
    assert_eq!(body(&vm.run().unwrap()), "ok 1");
}

#[test]
fn set_texts_none_returns_to_original_from_next_stop_point() {
    let prog = compile_ok("label start {\n  一 #a\n  二 #b\n  三 #c\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let t = table(&[
        ("main.start#a", "1"),
        ("main.start#b", "2"),
        ("main.start#c", "3"),
    ]);
    vm.set_texts(Some(t)).unwrap();
    assert_eq!(body(&vm.run().unwrap()), "1");
    // The current stop point does not change; from the next one, the original
    vm.set_texts(None).unwrap();
    assert_eq!(body(vm.current().unwrap()), "1");
    assert!(vm.texts().is_none());
    resume(&mut vm);
    assert_eq!(body(&vm.run().unwrap()), "二");
    // Setting it again gives translations from the next stop point
    vm.set_texts(Some(table(&[("main.start#c", "3")]))).unwrap();
    assert_eq!(body(vm.current().unwrap()), "二");
    resume(&mut vm);
    assert_eq!(body(&vm.run().unwrap()), "3");
}

// ---------------------------------------------------------------------------
// refresh_current
// ---------------------------------------------------------------------------

#[test]
fn refresh_current_rebuilds_current_stop_point_with_current_table() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert_eq!(body(&vm.run().unwrap()), "朝 7");
    vm.set_texts(Some(full_table())).unwrap();
    // set_texts alone does not change it
    assert_eq!(body(vm.current().unwrap()), "朝 7");
    let y = vm.refresh_current().unwrap();
    assert_eq!(body(&y), "Morning 7");
    assert_eq!(vm.current(), Some(&y));
    // Back to the original
    vm.set_texts(None).unwrap();
    assert_eq!(body(&vm.refresh_current().unwrap()), "朝 7");
    // Choice stop points are rebuilt too
    resume(&mut vm);
    vm.run().unwrap();
    resume(&mut vm);
    vm.run().unwrap();
    resume(&mut vm);
    let y = vm.run().unwrap();
    assert_eq!(args_of(&y)[0].to_text(), "[「はい 7」, 「いいえ」]");
    vm.set_texts(Some(full_table())).unwrap();
    let y = vm.refresh_current().unwrap();
    assert_eq!(args_of(&y)[0].to_text(), "[「Yes 7」, 「No」]");
}

#[test]
fn refresh_current_does_not_reevaluate_and_keeps_position_history_and_save() {
    let prog = compile_ok(
        "label start {\n  乱数 ${random(1, 1000000)} と ${random(1, 1000000)} #a\n  次 ${random(1, 1000000)} #b\n}\n",
    );
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let first = body(&vm.run().unwrap());
    let nums: Vec<&str> = first.split_whitespace().collect();
    vm.set_texts(Some(table(&[("main.start#a", "R ${2}, ${1}")])))
        .unwrap();
    let saved = vm.save().unwrap();
    let history = vm.history().to_vec();
    let vars = vm.variables();
    let y = vm.refresh_current().unwrap();
    // Only reordering the evaluated values (random numbers), without drawing new random numbers
    assert_eq!(body(&y), format!("R {}, {}", nums[3], nums[1]));
    assert_eq!(vm.save().unwrap(), saved);
    assert_eq!(vm.history(), history.as_slice());
    assert_eq!(vm.variables(), vars);
    // The same no matter how many times it is called, and it does not affect the later random sequence
    assert_eq!(vm.refresh_current().unwrap(), y);
    let mut plain = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    plain.run().unwrap();
    resume(&mut plain);
    let expected_next = body(&plain.run().unwrap());
    resume(&mut vm);
    assert_eq!(body(&vm.run().unwrap()), expected_next);
}

#[test]
fn refresh_current_works_right_after_load() {
    let src =
        "save var n: int = 5\nlabel start {\n  朝 ${n} と ${random(1, 1000000)} #a\n  次 #b\n}\n";
    let prog = compile_ok(src);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let orig = body(&vm.run().unwrap());
    let bytes = vm.save().unwrap();
    let (mut loaded, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    // The values rebuilt by the load (including random numbers) can be used as they are
    assert_eq!(body(loaded.current().unwrap()), orig);
    loaded
        .set_texts(Some(table(&[("main.start#a", "N=${1} R=${2}")])))
        .unwrap();
    let y = loaded.refresh_current().unwrap();
    let words: Vec<&str> = orig.split_whitespace().collect();
    assert_eq!(body(&y), format!("N=5 R={}", words[3]));
    // The loaded history stays as the original
    assert_eq!(
        loaded.history().last().unwrap().args,
        vec![Value::Str(orig.into())]
    );
}

#[test]
fn refresh_current_returns_same_notification_for_stop_points_without_body() {
    let prog = compile_ok("label start {\n  @game.give(\"薬\", 2) #g\n  あ #a\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    vm.set_texts(Some(table(&[("main.start#g", "x"), ("main.start#a", "y")])))
        .unwrap();
    assert_eq!(vm.refresh_current().unwrap(), y);
}

#[test]
fn refresh_current_only_while_stopped() {
    let prog = compile_ok("label start {\n  あ #a\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    // Not run yet
    assert_eq!(
        vm.refresh_current().unwrap_err().kind,
        VmErrorKind::NotStopped
    );
    vm.run().unwrap();
    resume(&mut vm);
    // After resuming
    assert_eq!(
        vm.refresh_current().unwrap_err().kind,
        VmErrorKind::NotStopped
    );
    assert!(matches!(vm.run().unwrap(), Yield::End));
    // After the end
    assert_eq!(
        vm.refresh_current().unwrap_err().kind,
        VmErrorKind::NotStopped
    );
}

#[test]
fn history_keeps_original_bodies_and_values_after_refresh_current_and_resume() {
    let prog = compile_ok("save var n: int = 4\nlabel start {\n  朝 ${n} #a\n}\n");
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[("main.start#a", "Day ${1}")])))
        .unwrap();
    vm.run().unwrap();
    resume(&mut vm);
    let r = &vm.history()[0];
    assert_eq!(r.args, vec![Value::Str("朝 4".into())]);
    assert_eq!(r.values, vec![vec!["4".to_string()]]);
    assert_eq!(r.kind, StopKind::Plain);
    assert_eq!(r.label, "main.start");
    assert_eq!(r.key.as_deref(), Some("main.start#a"));
}

// ---------------------------------------------------------------------------
// Rendering history in the current language
// ---------------------------------------------------------------------------

#[test]
fn history_can_be_rendered_in_current_language() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    for _ in 0..5 {
        resume(&mut vm);
        vm.run().unwrap();
    }
    let h: Vec<Arc<StopRecord>> = vm.history().to_vec();
    assert_eq!(h.len(), 6);
    // Without a table, the original
    assert_eq!(vm.history_text(&h[0]).as_deref(), Some("朝 7"));
    assert_eq!(vm.render(&h[0]), h[0].args);
    vm.set_texts(Some(full_table())).unwrap();
    assert_eq!(vm.history_text(&h[0]).as_deref(), Some("Morning 7"));
    assert_eq!(vm.history_text(&h[1]).as_deref(), Some("「Hi 7」"));
    // Lines without a translation or without an ID are the original
    assert_eq!(vm.history_text(&h[2]).as_deref(), Some("ID なし"));
    // For a choice, `render` gives the list of option texts (`history_text` is `None`)
    assert_eq!(vm.history_text(&h[3]), None);
    assert_eq!(vm.render(&h[3])[0].to_text(), "[「Yes 7」, 「No」]");
    // The line chosen
    assert_eq!(vm.history_text(&h[4]).as_deref(), Some("Yes reply"));
    // `@` commands are `None` / as is
    assert_eq!(vm.history_text(&h[5]), None);
    assert_eq!(vm.render(&h[5]), h[5].args);
    // The history itself does not change
    assert_eq!(vm.history()[0].args, vec![Value::Str("朝 7".into())]);
    vm.set_texts(None).unwrap();
    assert_eq!(vm.history_text(&h[0]).as_deref(), Some("朝 7"));
}

#[test]
fn loaded_history_can_be_rendered_in_current_language() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    for _ in 0..3 {
        vm.run().unwrap();
        resume(&mut vm);
    }
    let y = vm.run().unwrap();
    assert!(matches!(&args_of(&y)[0], Value::List(_)));
    let bytes = vm.save().unwrap();
    let (mut loaded, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    loaded.set_texts(Some(full_table())).unwrap();
    let h = loaded.history().to_vec();
    assert_eq!(h.len(), 4);
    assert_eq!(loaded.history_text(&h[0]).as_deref(), Some("Morning 7"));
    assert_eq!(loaded.history_text(&h[1]).as_deref(), Some("「Hi 7」"));
    // The history of a choice can also be rendered after a load, because the label is known
    assert_eq!(loaded.render(&h[3])[0].to_text(), "[「Yes 7」, 「No」]");
    assert_eq!(h[3].kind, StopKind::Choice);
}

// ---------------------------------------------------------------------------
// Language has nothing to do with saves, positions or rollback
// ---------------------------------------------------------------------------

#[test]
fn language_is_not_in_save_and_table_presence_does_not_change_save() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut a = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut b = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    b.set_texts(Some(full_table())).unwrap();
    for _ in 0..4 {
        a.run().unwrap();
        b.run().unwrap();
        assert_eq!(a.save().unwrap(), b.save().unwrap());
        assert_eq!(a.history(), b.history());
        resume(&mut a);
        resume(&mut b);
    }
}

#[test]
fn save_and_load_around_setting_table_keep_position() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    resume(&mut vm);
    vm.run().unwrap();
    let before = vm.save().unwrap();
    vm.set_texts(Some(full_table())).unwrap();
    vm.refresh_current().unwrap();
    let after = vm.save().unwrap();
    assert_eq!(before, after);
    // Loading a save of a VM with originals into a VM with translations gives the same stop point
    let (mut loaded, report) = Vm::load(&prog, &reg, VmConfig::default(), &before).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    loaded.set_texts(Some(full_table())).unwrap();
    assert_eq!(
        loaded.refresh_current().unwrap(),
        vm.current().cloned().unwrap()
    );
    // The sequence of following stop points is the same as the original (positions are the same with translations too)
    let mut orig = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    orig.run().unwrap();
    resume(&mut orig);
    orig.run().unwrap();
    let rest_orig: Vec<String> = all_bodies(&mut orig);
    let rest_tr: Vec<String> = all_bodies(&mut vm);
    assert_eq!(rest_orig.len(), rest_tr.len());
}

#[test]
fn stop_point_restored_by_rollback_is_rebuilt_in_current_language() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap = vm.snapshot().unwrap();
    resume(&mut vm);
    vm.run().unwrap();
    // Take a snapshot with the original, switch to translations, then restore
    vm.set_texts(Some(full_table())).unwrap();
    vm.restore(&snap).unwrap();
    assert_eq!(body(vm.current().unwrap()), "Morning 7");
    assert_eq!(
        vm.history().last().unwrap().args,
        vec![Value::Str("朝 7".into())]
    );
    // Take a snapshot with translations, switch back to the original, then restore
    let snap_tr = vm.snapshot().unwrap();
    vm.set_texts(None).unwrap();
    vm.restore(&snap_tr).unwrap();
    assert_eq!(body(vm.current().unwrap()), "朝 7");
    assert_eq!(
        vm.refresh_current().unwrap(),
        vm.current().cloned().unwrap()
    );
}

#[test]
fn stop_point_rebuilt_by_persistent_load_uses_current_language_and_history_stays_original() {
    let reg = registry();
    let prog = compile_ok("persistent var p: int = 1\nlabel start {\n  値 ${p} #a\n}\n");
    // Make persistent data (`p = 42`) from a script that differs only in the initial value of `p`
    let other = compile_ok("persistent var p: int = 42\nlabel start {\n  値 ${p} #a\n}\n");
    let mut o = Vm::new(&other, &reg, VmConfig::default()).unwrap();
    o.run().unwrap();
    let persistent = o.save_persistent();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[("main.start#a", "value ${1}")])))
        .unwrap();
    vm.run().unwrap();
    assert_eq!(body(vm.current().unwrap()), "value 1");
    vm.load_persistent(&persistent).unwrap();
    // The stop point is rebuilt with the new value and shown in the current language. The history stays as the original
    assert_eq!(body(vm.current().unwrap()), "value 42");
    assert_eq!(
        vm.history().last().unwrap().args,
        vec![Value::Str("値 42".into())]
    );
    assert_eq!(
        vm.refresh_current().unwrap(),
        vm.current().cloned().unwrap()
    );
}

// ---------------------------------------------------------------------------
// Table checks, templates and files
// ---------------------------------------------------------------------------

#[test]
fn table_check_returns_missing_extra_and_bad_indices() {
    let prog = compile_ok(SRC);
    let t = table(&[
        ("identity:main.案内人", "Imouto"),
        ("main.start#a", "x ${1} ${2}"),
        ("main.start#b", "y"),
        ("main.start#yes", "z"),
        ("main.start#gone", "余り"),
        ("main.other#zz", "余り 2"),
    ]);
    let issues = t.check(&prog);
    let shown: Vec<(String, &str, crate::Severity)> = issues
        .iter()
        .map(|i| (i.key.clone(), i.kind.code(), i.severity))
        .collect();
    assert_eq!(
        shown,
        vec![
            (
                "main.start#a".to_string(),
                "bad_number",
                crate::Severity::Error
            ),
            (
                "main.start#no".to_string(),
                "missing",
                crate::Severity::Warning
            ),
            (
                "main.start#e1".to_string(),
                "missing",
                crate::Severity::Warning
            ),
            (
                "main.start#e2".to_string(),
                "missing",
                crate::Severity::Warning
            ),
            (
                "main.start#gone".to_string(),
                "unused",
                crate::Severity::Warning
            ),
            (
                "main.other#zz".to_string(),
                "unused",
                crate::Severity::Warning
            ),
        ]
    );
    assert_eq!(
        issues[0].kind,
        TextIssueKind::BadNumber {
            number: 2,
            count: 1
        }
    );
    // A complete table has no problems
    assert!(full_table_all().check(&prog).is_empty());
}

fn full_table_all() -> TextTable {
    let mut t = full_table();
    t.insert("identity:main.案内人", "Imouto").unwrap();
    t.insert("main.start#no", "「No」").unwrap();
    t.insert("main.start#e2", "No reply").unwrap();
    t
}

#[test]
fn template_is_table_with_numbered_interpolations_that_can_be_set_as_is() {
    let prog = compile_ok(
        "save var n: int = 7\nlabel start {\n  $5 の \\${} と ${n} と ${n + 1} \\\\ #a\n  ID なし\n  choice {\n    「はい ${n}」 #y => jump start\n  }\n}\n",
    );
    let t = TextTable::template(&prog, "xx");
    assert_eq!(t.language(), "xx");
    assert_eq!(t.len(), 2);
    assert_eq!(
        t.get("main.start#a"),
        Some("$5 の \\${} と ${1} と ${2} \\\\")
    );
    assert_eq!(t.get("main.start#y"), Some("「はい ${1}」"));
    assert!(t.check(&prog).is_empty());
    // Setting it gives the same bodies as the original
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let orig = body(&vm.run().unwrap());
    let mut vm2 = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm2.set_texts(Some(t)).unwrap();
    assert_eq!(body(&vm2.run().unwrap()), orig);
}

#[test]
fn table_file_round_trips() {
    let mut t = TextTable::new("en");
    t.insert("main.start#a", "改行\n\\n と タブ\t と \\\\ と ${1}")
        .unwrap_err();
    t.insert("main.start#a", "改行\nと タブ\t と \\\\ と ${1}")
        .unwrap();
    t.insert("main.start#b", "「b」").unwrap();
    let file = t.to_file();
    assert_eq!(
        file,
        "@lang en\nmain.start#a\t改行\\nと タブ\\t と \\\\ と ${1}\nmain.start#b\t「b」\n"
    );
    let back = TextTable::from_file(&file).unwrap();
    assert_eq!(back, t);
    assert_eq!(back.language(), "en");
}

#[test]
fn table_file_skips_empty_lines_comments_crlf_and_bom() {
    let src = "\u{feff}# コメント\r\n\r\n@lang  ja \r\nmain.start#a\tあ\r\n# 途中のコメント\r\nmain.start#b\tい#う\r\n";
    let t = TextTable::from_file(src).unwrap();
    assert_eq!(t.language(), "ja");
    assert_eq!(t.get("main.start#a"), Some("あ"));
    // `#` is a comment only at the start of a line
    assert_eq!(t.get("main.start#b"), Some("い#う"));
}

#[test]
fn table_file_errors_have_lines() {
    let e = TextTable::from_file("@lang en\nno tab here\n").unwrap_err();
    assert_eq!(
        (e.kind.clone(), e.line),
        (TextErrorKind::MissingTab, Some(2))
    );
    let e = TextTable::from_file("\ta\n").unwrap_err();
    assert_eq!((e.kind.clone(), e.line), (TextErrorKind::EmptyKey, Some(1)));
    let e = TextTable::from_file("k\ta\nk\tb\n").unwrap_err();
    assert_eq!(
        (e.kind.clone(), e.line, e.key.as_deref()),
        (TextErrorKind::DuplicateKey, Some(2), Some("k"))
    );
    let e = TextTable::from_file("@plural en\n").unwrap_err();
    assert_eq!(
        e.kind,
        TextErrorKind::UnknownDirective {
            name: "plural".to_string()
        }
    );
    let e = TextTable::from_file("k\t${x}\n").unwrap_err();
    assert_eq!((e.kind.code(), e.line), ("invalid_placeholder", Some(1)));
}

// ---------------------------------------------------------------------------
// require_text_ids
// ---------------------------------------------------------------------------

#[test]
fn require_text_ids_makes_body_lines_without_ids_errors() {
    let reg = registry();
    let src = "identity 案内人 = \"案内人\"\nlabel start {\n  素\n  案内人「引用」\n  素 #a\n  @game.give(\"c\", 1)\n  choice {\n    「甲」 => jump start\n    「乙」 #b => jump start\n  }\n}\n";
    // Disabled by default
    assert!(
        compile_modules_with_options(&[("main", src)], &reg, &CompileOptions::default()).is_ok()
    );
    let opts = CompileOptions::default().require_text_ids(true);
    let errors = compile_modules_with_options(&[("main", src)], &reg, &opts).unwrap_err();
    let found: Vec<(u32, &str)> = errors
        .iter()
        .map(|d| {
            assert_eq!(d.code, DiagCode::MissingTextId);
            (d.line, d.arg_str("kind").unwrap())
        })
        .collect();
    assert_eq!(found, vec![(3, "plain"), (4, "quote"), (8, "choice")]);
    // Passes if everything has an ID
    let ok = "label start {\n  素 #a\n  choice {\n    「甲」 #b => jump start\n  }\n}\n";
    assert!(compile_modules_with_options(&[("main", ok)], &reg, &opts).is_ok());
}
