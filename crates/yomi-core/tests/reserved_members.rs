//! Tests from the public API that reserved words can be used as the name after `namespace.` (section 1.6 of
//! `spec/language/01-lexical.md`).

use yomi_core::{
    Ctx, Input, Kind, Package, PackageReg, Registry, RegistryError, Sugar, Value, Vm, VmConfig,
    Yield, compile,
};

struct Game;

impl Package for Game {
    const NAME: &'static str = "game";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("game.int", |_: &mut Ctx<Game>, _n: i64| {});
        reg.query("game.map", |_: &Ctx<Game>| -> i64 { 7 });
        reg.var::<i64>("game.end", Kind::Save, 5);
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.package::<Game>().unwrap();
    r
}

#[test]
fn keywords_can_be_registered_as_names_after_namespace() {
    let mut r = Registry::new();
    assert!(r.stop::<(), ()>("input.int").is_ok());
    assert!(r.stop::<(), ()>("input.in").is_ok());
    assert!(r.var::<i64>("game.end", Kind::Save, 0).is_ok());
    assert!(r.alias("old.map", "input.int").is_ok());
}

#[test]
fn keywords_cannot_be_namespaces_or_unqualified_names() {
    let mut r = Registry::new();
    for name in ["if.x", "int", "end", "list.if.x"] {
        assert!(
            matches!(r.stop::<(), ()>(name), Err(RegistryError::InvalidName(_))),
            "{name}"
        );
    }
    assert!(matches!(
        r.var::<i64>("map.x", Kind::Save, 0),
        Err(RegistryError::InvalidName(_))
    ));
}

#[test]
fn scripts_can_use_commands_and_variables_with_keyword_names() {
    let reg = registry();
    let src = "import game\nlabel start {\n  game.end += 1\n  @game.int(4)\n  ${game.end} ${game.map()}\n}\n";
    let prog = compile(src, "main", &reg).unwrap_or_else(|d| panic!("{d:?}"));
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut out = Vec::new();
    loop {
        match vm.run().unwrap() {
            Yield::End => break,
            Yield::Stop { args, .. } => {
                out.push(args.last().map(Value::to_text).unwrap_or_default());
                vm.resume(Input::Return(Value::Unit)).unwrap();
            }
            _ => {}
        }
    }
    assert_eq!(out, ["6 7"]);
}

#[test]
fn keywords_cannot_be_used_as_script_declaration_names() {
    let reg = registry();
    for bad in [
        "label start {\n  let end = 1\n}\n",
        "label start {\n  let map = 1\n}\n",
        "save var int: int = 0\nlabel start {\n  x\n}\n",
        "label map {\n  x\n}\n",
        "struct S {\n  in: int\n}\nlabel start {\n  x\n}\n",
    ] {
        assert!(compile(bad, "main", &reg).is_err(), "{bad}");
    }
}
