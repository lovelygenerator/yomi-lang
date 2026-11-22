//! Return values of commands

use super::*;

#[test]
fn map_and_identity_can_be_received_by_let_and_assignment() {
    let src = "save var best: map<str, int> = map {}\nlabel start {\n  let s = @game.scores()\n  ${s} ${s.len()}\n  best = @game.scores()\n  let w = @game.who()\n  ${w.id} ${w.name}\n  @game.show(s)\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    // game.scores
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    let scores = YomiMap(vec![("b".to_string(), 2i64), ("a".to_string(), 1)]);
    vm.resume(Input::Return(crate::registry::IntoValue::into_value(
        scores,
    )))
    .unwrap();
    let y = vm.run().unwrap();
    assert_eq!(text_of(&y), "map { b: 2, a: 1 } 2");
    resume(&mut vm);
    // best = game.scores()
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    vm.resume(Input::Return(Value::Map(
        vec![(Value::Str("z".into()), Value::Int(9))].into(),
    )))
    .unwrap();
    // game.who
    assert!(matches!(vm.run().unwrap(), Yield::Stop { .. }));
    vm.resume(Input::Return(Value::Identity(std::sync::Arc::new(
        Identity {
            id: "案内人".into(),
            name: "あんないにん".into(),
        },
    ))))
    .unwrap();
    let y = vm.run().unwrap();
    assert_eq!(text_of(&y), "案内人 あんないにん");
    resume(&mut vm);
    // game.show(s): the argument is a map
    let y = vm.run().unwrap();
    match y {
        Yield::Stop { args, .. } => assert_eq!(
            args,
            vec![Value::Map(
                vec![
                    (Value::Str("b".into()), Value::Int(2)),
                    (Value::Str("a".into()), Value::Int(1)),
                ]
                .into()
            )]
        ),
        other => panic!("{other:?}"),
    }
    resume(&mut vm);
    assert!(matches!(vm.run().unwrap(), Yield::End));
    assert_eq!(var(&vm, "best").to_text(), "map { z: 9 }");
}

#[test]
fn resume_rejects_map_return_value_of_wrong_type() {
    let src = "label start {\n  let s = @game.scores()\n}\n";
    let reg = registry();
    let prog = compile_ok(src);
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bad = Value::Map(vec![(Value::Str("a".into()), Value::Str("x".into()))].into());
    let e = vm.resume(Input::Return(bad)).unwrap_err();
    assert_eq!(e.kind.code(), "bad_input");
}

#[test]
fn yomimap_keeps_insertion_order_and_updates_duplicate_keys() {
    use crate::registry::{FromValue, IntoValue};
    let m = YomiMap(vec![
        ("b".to_string(), 1i64),
        ("a".to_string(), 2),
        ("b".to_string(), 3),
    ]);
    let v = m.into_value();
    assert_eq!(v.to_text(), "map { b: 3, a: 2 }");
    let back = <YomiMap<String, i64>>::from_value(&v).unwrap();
    assert_eq!(back.0, vec![("b".to_string(), 3), ("a".to_string(), 2)]);
    assert_eq!(
        <YomiMap<String, i64> as crate::registry::YomiType>::ty().name(),
        "map<str, int>"
    );
}

#[test]
fn host_declared_variables_can_be_map() {
    let mut r = registry();
    assert!(
        r.var::<YomiMap<String, i64>>("host.m", Kind::Save, YomiMap(vec![]))
            .is_ok()
    );
}
