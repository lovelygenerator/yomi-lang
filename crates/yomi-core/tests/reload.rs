//! Handing a stopped VM over to a new program (`spec/host/04-save.md`, 4.10).

use yomi_core::{
    Input, Registry, ReloadError, ResolvedBy, Sugar, Value, Vm, VmConfig, Yield, compile,
};

fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r
}

const V1: &str = "save var n: int = 0\nlabel start {\n  n = n + 1\n  一行目 #one\n  二行目 #two\n  三行目 #three\n}\n";
const V2: &str = "save var n: int = 0\nlabel start {\n  n = n + 1\n  一行目 #one\n  直した二行目 #two\n  三行目 #three\n}\n";

fn text(y: &Yield) -> Value {
    match y {
        Yield::Stop { args, .. } => args[0].clone(),
        _ => panic!("not a stop"),
    }
}

fn at_two<'a>(program: &'a yomi_core::Program, reg: &'a Registry) -> Vm<'a> {
    let mut vm = Vm::new(program, reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    vm
}

#[test]
fn reload_keeps_the_stop_point_and_shows_the_new_text() {
    let reg = registry();
    let p1 = compile(V1, "main", &reg).unwrap();
    let p2 = compile(V2, "main", &reg).unwrap();
    let vm = at_two(&p1, &reg);
    let (mut next, report) = vm.reload(&p2).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Id);
    assert_eq!(
        text(next.current().unwrap()),
        Value::Str("直した二行目".into())
    );
    assert!(vm.is_stopped(), "the old VM is untouched");
    next.resume(Input::Return(Value::Unit)).unwrap();
    assert_eq!(text(&next.run().unwrap()), Value::Str("三行目".into()));
    let n = next
        .variables()
        .into_iter()
        .find(|v| v.name.ends_with('n'))
        .unwrap();
    assert_eq!(n.value, Value::Int(1));
}

#[test]
fn reload_keeps_the_history() {
    let reg = registry();
    let p1 = compile(V1, "main", &reg).unwrap();
    let p2 = compile(V2, "main", &reg).unwrap();
    let vm = at_two(&p1, &reg);
    let (next, _) = vm.reload(&p2).unwrap();
    assert_eq!(next.history().len(), vm.history().len());
}

#[test]
fn reload_fails_when_the_label_is_gone() {
    let reg = registry();
    let p1 = compile(V1, "main", &reg).unwrap();
    let p2 = compile(
        "save var n: int = 0\nlabel other {\n  別の行 #zzz\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let vm = at_two(&p1, &reg);
    let err = vm.reload(&p2).err().unwrap();
    assert!(
        matches!(err, ReloadError::PositionLost(_) | ReloadError::Load(_)),
        "{err:?}"
    );
    assert!(vm.is_stopped());
    assert_eq!(text(vm.current().unwrap()), Value::Str("二行目".into()));
}

#[test]
fn reload_fails_when_a_variable_changes_type_under_strict() {
    let reg = registry();
    let p1 = compile(V1, "main", &reg).unwrap();
    let p2 = compile(
        &V1.replace("n: int = 0", "n: str = \"\"")
            .replace("n = n + 1", "n = \"x\""),
        "main",
        &reg,
    )
    .unwrap();
    let vm = at_two(&p1, &reg);
    assert!(vm.reload(&p2).is_err());
}

#[test]
fn reload_needs_a_stop() {
    let reg = registry();
    let p1 = compile(V1, "main", &reg).unwrap();
    let vm = Vm::new(&p1, &reg, VmConfig::default()).unwrap();
    assert_eq!(vm.reload(&p1).err(), Some(ReloadError::NotStopped));
}

#[test]
fn reload_keeps_persistent_data() {
    let reg = registry();
    let src = "persistent var p: int = 0\nlabel start {\n  p = p + 5\n  行 #a\n  次 #b\n}\n";
    let p1 = compile(src, "main", &reg).unwrap();
    let mut vm = Vm::new(&p1, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let p2 = compile(src, "main", &reg).unwrap();
    let (next, _) = vm.reload(&p2).unwrap();
    let p = next
        .variables()
        .into_iter()
        .find(|v| v.name.ends_with('p'))
        .unwrap();
    assert_eq!(p.value, Value::Int(5));
}

#[test]
fn reload_resolves_by_ordinal_when_the_id_and_text_are_gone() {
    let reg = registry();
    let p1 = compile(V1, "main", &reg).unwrap();
    let p2 = compile(
        "save var n: int = 0\nlabel start {\n  n = n + 1\n  甲 #a\n  乙 #b\n  丙 #c\n}\n",
        "main",
        &reg,
    )
    .unwrap();
    let vm = at_two(&p1, &reg);
    let (next, report) = vm.reload(&p2).unwrap();
    assert_eq!(report.resolved_by, ResolvedBy::Ordinal);
    assert_eq!(text(next.current().unwrap()), Value::Str("乙".into()));
}

#[test]
fn reload_fails_with_position_lost_when_a_local_changes_type() {
    let reg = registry();
    let a = "label start {\n  let x = 1\n  一 #a\n  二 #b\n}\n";
    let b = "label start {\n  let x = \"s\"\n  一 #a\n  二 #b\n}\n";
    let p1 = compile(a, "main", &reg).unwrap();
    let p2 = compile(b, "main", &reg).unwrap();
    let vm = at_two(&p1, &reg);
    assert!(matches!(
        vm.reload(&p2).err(),
        Some(ReloadError::PositionLost(ResolvedBy::LabelStart))
    ));
    assert!(vm.is_stopped());
}
