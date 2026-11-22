//! Bulk registration

use super::*;

use yomi_core::packages;

/// How the registry of state "looks" (to compare that it has not changed).
pub(super) fn shape(reg: &Registry) -> (Vec<String>, Vec<(String, Kind)>) {
    (
        reg.packages().map(|p| p.name).collect(),
        reg.vars()
            .iter()
            .map(|v| (v.name.clone(), v.kind))
            .collect(),
    )
}

#[test]
fn package_set_registers_dependencies_first_even_if_written_later() {
    let mut reg = host_registry();
    reg.package_set(&packages![Mirror, Other, Counter]).unwrap();
    let names: Vec<String> = reg.packages().map(|p| p.name).collect();
    // Packages unrelated by dependencies keep the given order (mirror goes after counter)
    assert_eq!(names, ["other", "counter", "mirror"]);
    assert_eq!(reg.command_kind("mirror.copy"), Some(CommandKind::Effect));
    // Even after reordering, they can be used from scripts
    let prog = compile_ok(
        "import counter\nimport mirror\nlabel start {\n  @counter.inc(4)\n  @mirror.copy()\n  ${mirror.double()}\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    match vm.run().unwrap() {
        Yield::Stop { args, .. } => assert_eq!(args, [Value::Str("8".into())]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn package_set_dependencies_may_be_already_registered() {
    let mut reg = host_registry();
    reg.package::<Counter>().unwrap();
    reg.package_set(&packages![Mirror]).unwrap();
    assert_eq!(reg.packages().count(), 2);
    // An empty set does nothing
    reg.package_set(&[]).unwrap();
    assert_eq!(reg.packages().count(), 2);
}

#[test]
fn package_set_keeps_registry_when_dependency_is_missing() {
    let mut reg = host_registry();
    reg.package::<Other>().unwrap();
    let before = shape(&reg);
    assert_eq!(
        reg.package_set(&packages![Cued, Mirror]),
        Err(RegistryError::MissingDependency {
            package: "mirror".to_string(),
            dependency: "counter".to_string()
        })
    );
    assert_eq!(shape(&reg), before);
    assert_eq!(reg.command_kind("cued.cue"), None);
    // Even after a failure, the same name can be registered
    reg.package_set(&packages![Cued, Counter, Mirror]).unwrap();
}

#[test]
fn package_set_rejects_dependency_cycles_including_self_and_keeps_registry() {
    let mut reg = host_registry();
    reg.package::<Other>().unwrap();
    let before = shape(&reg);
    let e = reg.package_set(&packages![Cued, DepA, DepB]).unwrap_err();
    assert!(matches!(e, RegistryError::DependencyCycle(_)), "{e:?}");
    assert_eq!(shape(&reg), before);
    assert_eq!(
        reg.package_set(&packages![Cued, SelfDependent]),
        Err(RegistryError::DependencyCycle("selfdep".to_string()))
    );
    assert_eq!(shape(&reg), before);
    assert_eq!(reg.command_kind("cued.cue"), None);
}

#[test]
fn package_set_keeps_registry_when_a_package_fails_midway() {
    let mut reg = host_registry();
    reg.package::<Other>().unwrap();
    let before = shape(&reg);
    // The second, `Stray`, fails with `InvalidName`. The first, `Counter`, does not remain either
    assert_eq!(
        reg.package_set(&packages![Counter, Stray]),
        Err(RegistryError::InvalidName("elsewhere.bad".to_string()))
    );
    assert_eq!(shape(&reg), before);
    assert_eq!(reg.command_kind("counter.inc"), None);
}

#[test]
fn package_set_rejects_duplicate_names() {
    let mut reg = host_registry();
    reg.package::<Counter>().unwrap();
    assert_eq!(
        reg.package_set(&packages![Other, Counter]),
        Err(RegistryError::DuplicatePackage("counter".to_string()))
    );
    assert_eq!(reg.packages().count(), 1);
    let mut reg2 = host_registry();
    assert_eq!(
        reg2.package_set(&packages![Other, Other]),
        Err(RegistryError::DuplicatePackage("other".to_string()))
    );
    assert_eq!(reg2.packages().count(), 0);
}
