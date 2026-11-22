//! Tests of strings other than bodies that text tables can translate (display names of identities, and `text_param`
//! arguments in the registry such as input prompts) (section 5.2a of `spec/host/05-texts.md`).

use super::util::{self, *};
use crate::compile::{CompileOptions, compile_modules_with_options};
use crate::diag::DiagCode;
use crate::program::Program;
use crate::registry::{RegistryError, Sugar};
use crate::texts::TextIssueKind;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Quote], |r| {
        r.stop::<(String,), String>("game.ask").unwrap();
        r.stop::<(String, String), String>("game.ask2").unwrap();
        r.stop::<(String,), ()>("game.give").unwrap();
        r.text_param("game.ask", 0).unwrap();
        r.text_param("game.ask2", 1).unwrap();
    })
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

const SRC: &str = "identity 案内人 = \"リオン\"\nidentity 村長 = \"旅の人\"\nsave var n: int = 7\nlabel start {\n  案内人「やあ」 #a\n  村長「ID なし」\n  let a = @game.ask(\"名前は？ ${n}\") #ask\n  let b = @game.ask(\"ID なし\")\n  let c = @game.ask2(\"${n}番\", \"${n}回目\") #ask2\n  let d = \"動的\"\n  let e = @game.ask(d) #dyn\n  @game.give(\"薬\") #give\n}\n";

fn full_table() -> TextTable {
    table(&[
        ("identity:main.案内人", "Rion"),
        ("identity:main.村長", "Village Chief"),
        ("main.start#ask", "Your name? ${1}"),
        ("main.start#ask2", "Take ${1}"),
        ("main.start#dyn", "訳"),
        ("main.start#give", "Medicine"),
    ])
}

fn who(name: &str, id: &str) -> Value {
    Value::Option(Some(Box::new(Value::Identity(std::sync::Arc::new(
        crate::Identity {
            id: id.to_string(),
            name: name.to_string(),
        },
    )))))
}

/// Makes a value to return that fits the stop point, and resumes.
fn answer(vm: &mut Vm<'_>) {
    let v = match vm.current() {
        Some(Yield::Stop { command, .. }) => match vm.registry.command(*command).name.as_str() {
            "game.ask" | "game.ask2" => Value::Str("x".into()),
            _ => Value::Unit,
        },
        _ => Value::Unit,
    };
    vm.resume(Input::Return(v)).unwrap();
}

/// Collects the arguments of all stop points in order.
fn all_args(vm: &mut Vm<'_>) -> Vec<Vec<Value>> {
    let mut out = Vec::new();
    loop {
        match vm.run().unwrap() {
            Yield::End => return out,
            y => {
                out.push(args_of(&y));
                answer(vm);
            }
        }
    }
}

fn s(t: &str) -> Value {
    Value::Str(t.into())
}

#[test]
fn identity_display_names_and_translatable_args_are_translated_by_table() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(full_table())).unwrap();
    let got = all_args(&mut vm);
    assert_eq!(
        got,
        vec![
            // Quote lines: the identity's display name is translated (the id stays). The body is the original if not in the table
            vec![who("Rion", "案内人"), s("「やあ」")],
            // Even if the line has no ID, the identity's display name is translated
            vec![who("Village Chief", "村長"), s("「ID なし」")],
            // Input prompts: string literals with an ID are translated (the values of `${n}` are as in the original)
            vec![s("Your name? 7")],
            // Lines without an ID are the original
            vec![s("ID なし")],
            // The argument number and the number of the `${expr}` value match (only the second argument is translated)
            vec![s("7番"), s("Take 7")],
            // Prompts that are not string literals stay the original even if the table has the key
            vec![s("動的")],
            // Arguments of commands without `text_param` stay the original even if the table has the key
            vec![s("薬")],
        ]
    );
}

#[test]
fn without_table_or_key_uses_original() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let plain = all_args(&mut vm);
    assert_eq!(plain[0], vec![who("リオン", "案内人"), s("「やあ」")]);
    assert_eq!(plain[2], vec![s("名前は？ 7")]);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(table(&[("identity:main.案内人", "Rion")])))
        .unwrap();
    let got = all_args(&mut vm);
    assert_eq!(got[0], vec![who("Rion", "案内人"), s("「やあ」")]);
    assert_eq!(got[1], vec![who("旅の人", "村長"), s("「ID なし」")]);
    assert_eq!(got[2], plain[2]);
}

#[test]
fn translated_names_and_prompts_apply_to_current_stop_point_via_refresh_current_without_reevaluating()
 {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let y = vm.run().unwrap();
    assert_eq!(args_of(&y)[0], who("リオン", "案内人"));
    vm.set_texts(Some(full_table())).unwrap();
    // Only setting it does not change the current stop point
    assert_eq!(args_of(vm.current().unwrap())[0], who("リオン", "案内人"));
    let y = vm.refresh_current().unwrap();
    assert_eq!(args_of(&y), vec![who("Rion", "案内人"), s("「やあ」")]);
    assert_eq!(vm.current(), Some(&y));
    // Also at a prompt stop point
    answer(&mut vm);
    vm.run().unwrap();
    answer(&mut vm);
    let y = vm.run().unwrap();
    assert_eq!(args_of(&y), vec![s("Your name? 7")]);
    vm.set_texts(None).unwrap();
    assert_eq!(
        args_of(&vm.refresh_current().unwrap()),
        vec![s("名前は？ 7")]
    );
}

#[test]
fn history_identity_names_and_prompts_render_in_current_language() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    for _ in 0..5 {
        vm.run().unwrap();
        answer(&mut vm);
    }
    vm.run().unwrap();
    let h = vm.history().to_vec();
    assert_eq!(h[0].args, vec![who("リオン", "案内人"), s("「やあ」")]);
    vm.set_texts(Some(full_table())).unwrap();
    assert_eq!(vm.render(&h[0]), vec![who("Rion", "案内人"), s("「やあ」")]);
    assert_eq!(
        vm.render(&h[1]),
        vec![who("Village Chief", "村長"), s("「ID なし」")]
    );
    assert_eq!(vm.render(&h[2]), vec![s("Your name? 7")]);
    assert_eq!(vm.render(&h[3]), vec![s("ID なし")]);
    assert_eq!(vm.render(&h[4]), vec![s("7番"), s("Take 7")]);
    // The history itself stays as the original
    assert_eq!(vm.history()[2].args, vec![s("名前は？ 7")]);
    // The loaded history is also in the current language
    let bytes = vm.save().unwrap();
    let (mut loaded, _) = Vm::load(&prog, &reg, VmConfig::default(), &bytes).unwrap();
    loaded.set_texts(Some(full_table())).unwrap();
    let h = loaded.history().to_vec();
    assert_eq!(
        loaded.render(&h[0]),
        vec![who("Rion", "案内人"), s("「やあ」")]
    );
    assert_eq!(loaded.render(&h[2]), vec![s("Your name? 7")]);
    // Stop points rebuilt by reading again (prompts that are not string literals) can be rebuilt with the same notification
    let y = loaded.refresh_current().unwrap();
    assert_eq!(args_of(&y), vec![s("動的")]);
}

#[test]
fn table_presence_does_not_change_save_and_names_and_prompts_keep_original() {
    let prog = compile_ok(SRC);
    let reg = registry();
    let mut a = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut b = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    b.set_texts(Some(full_table())).unwrap();
    for _ in 0..6 {
        a.run().unwrap();
        b.run().unwrap();
        assert_eq!(a.save().unwrap(), b.save().unwrap());
        assert_eq!(a.history(), b.history());
        answer(&mut a);
        answer(&mut b);
    }
}

#[test]
fn table_check_and_template_include_identity_name_and_prompt_keys() {
    let prog = compile_ok(SRC);
    let t = TextTable::template(&prog, "xx");
    let keys: Vec<&str> = t.iter().map(|(k, _)| k).collect();
    // In declaration order of identities, then script order (prompts that are not string literals and commands without
    // `text_param` are not included)
    assert_eq!(
        keys,
        vec![
            "identity:main.案内人",
            "identity:main.村長",
            "main.start#a",
            "main.start#ask",
            "main.start#ask2",
        ]
    );
    assert_eq!(t.get("identity:main.案内人"), Some("リオン"));
    assert_eq!(t.get("main.start#ask"), Some("名前は？ ${1}"));
    // The original of the second argument (the `${n}` of the first argument is not counted)
    assert_eq!(t.get("main.start#ask2"), Some("${1}回目"));
    assert!(t.check(&prog).is_empty());
    // Setting it gives the same output as the original
    let reg = registry();
    let mut plain = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.set_texts(Some(t)).unwrap();
    assert_eq!(all_args(&mut plain), all_args(&mut vm));
}

#[test]
fn table_check_returns_missing_extra_and_bad_indices_for_identity_and_prompts() {
    let prog = compile_ok(SRC);
    let t = table(&[
        ("identity:main.案内人", "Rion ${1}"),
        ("main.start#ask", "Name"),
        ("main.start#ask2", "Take ${2}"),
        ("identity:main.門番", "余り"),
    ]);
    let shown: Vec<(String, &str)> = t
        .check(&prog)
        .iter()
        .map(|i| (i.key.clone(), i.kind.code()))
        .collect();
    assert_eq!(
        shown,
        vec![
            ("identity:main.案内人".to_string(), "bad_number"),
            ("identity:main.村長".to_string(), "missing"),
            ("main.start#a".to_string(), "missing"),
            ("main.start#ask2".to_string(), "bad_number"),
            ("identity:main.門番".to_string(), "unused"),
        ]
    );
    assert_eq!(
        t.check(&prog)[0].kind,
        TextIssueKind::BadNumber {
            number: 1,
            count: 0
        }
    );
    // A table with number errors cannot be set
    let reg = registry();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    assert!(vm.set_texts(Some(t)).is_err());
}

#[test]
fn identity_of_another_module_is_translated_with_module_qualified_key() {
    let reg = registry();
    let a = "identity 案内人 = \"リオン\"\n";
    let b = "import chara\nlabel start {\n  案内人「やあ」 #a\n  chara.案内人「やあ」 #b\n}\n";
    let (prog, _) = compile_modules_with_options(
        &[("chara", a), ("main", b)],
        &reg,
        &CompileOptions::default(),
    )
    .unwrap();
    let t = table(&[("identity:chara.案内人", "Rion")]);
    // There are no translations yet for the bodies of quote lines (`#a`, `#b`)
    let missing: Vec<String> = t.check(&prog).into_iter().map(|i| i.key).collect();
    assert_eq!(missing, vec!["main.start#a", "main.start#b"]);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("main.start")).unwrap();
    vm.set_texts(Some(t)).unwrap();
    let y = vm.run().unwrap();
    assert_eq!(args_of(&y)[0], who("Rion", "案内人"));
    answer(&mut vm);
    // The same declaration in the form `module.name`
    assert_eq!(args_of(&vm.run().unwrap())[0], who("Rion", "案内人"));
}

#[test]
fn require_text_ids_requires_ids_on_lines_with_translatable_prompt_literals() {
    let reg = registry();
    let src = "label start {\n  let a = @game.ask(\"名前は？\")\n  let d = \"x\"\n  let b = @game.ask(d)\n  @game.give(\"薬\")\n  let c = @game.ask2(\"p\", \"q\")\n}\n";
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
    // Prompts that are not string literals and commands without `text_param` are not requested
    assert_eq!(found, vec![(2, "command"), (6, "command")]);
}

#[test]
fn registry_rejects_invalid_text_param_registration() {
    let mut r = Registry::new();
    r.stop::<(String, i64), ()>("game.a").unwrap();
    assert!(matches!(
        r.text_param("game.none", 0),
        Err(RegistryError::UnknownCommand(_))
    ));
    // Arguments out of range or not `str`
    assert!(matches!(
        r.text_param("game.a", 2),
        Err(RegistryError::InvalidParams(_))
    ));
    assert!(matches!(
        r.text_param("game.a", 1),
        Err(RegistryError::InvalidParams(_))
    ));
    r.text_param("game.a", 0).unwrap();
    assert_eq!(
        r.command(r.command_id("game.a").unwrap()).text_param,
        Some(0)
    );
}
