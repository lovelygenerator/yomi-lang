//! Tests of the schema in saves and persistent data: `LoadPolicy`, schema mismatches and version checks.

use super::util::{self, *};
use crate::program::Program;
use crate::registry::Sugar;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Quote], |_| {})
}

fn compile_ok(src: &str) -> Program {
    util::compile_ok(src, &registry())
}

const OLD: &str = "struct P { hp: int }\nsave var p: P = P { hp: 5 }\nsave var n: int = 1\nsave var s: str = \"a\"\npersistent var best: int = 0\nlabel start {\n  n = 2\n  one #a\n  two\n}\n";
const NEW: &str = "struct P { hp: int, mp: int }\nsave var p: P = P { hp: 1, mp: 2 }\nsave var n: str = \"x\"\nsave var s: str = \"a\"\npersistent var best: str = \"-\"\nlabel start {\n  one #a\n  two\n}\n";

fn saved_old(reg: &Registry, prog: &Program) -> (Vec<u8>, Vec<u8>) {
    let mut vm = Vm::new(prog, reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    (vm.save().unwrap(), vm.save_persistent())
}

#[test]
fn strict_load_returns_every_mismatched_variable_with_expanded_types() {
    let reg = registry();
    let old = compile_ok(OLD);
    let new = compile_ok(NEW);
    let (bytes, _) = saved_old(&reg, &old);
    let e = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap_err();
    let LoadErrorKind::SchemaMismatch(diffs) = e.kind else {
        panic!("{e:?}");
    };
    let names: Vec<&str> = diffs.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["main.p", "main.n"]);
    assert_eq!(diffs[1].saved, "int");
    assert_eq!(diffs[1].current, "str");
    assert!(diffs[0].saved.contains("hp: int") && !diffs[0].saved.contains("mp"));
    assert!(diffs[0].current.contains("mp: int"));
}

#[test]
fn lenient_load_resets_mismatched_variables_and_goes_back_to_label_start() {
    let reg = registry();
    let old = compile_ok(OLD);
    let new = compile_ok(NEW);
    let (bytes, _) = saved_old(&reg, &old);
    let cfg = VmConfig::default().load_policy(LoadPolicy::Lenient);
    let (vm, report) = Vm::load(&new, &reg, cfg, &bytes).unwrap();
    assert_eq!(report.type_changed_vars, ["main.p", "main.n"]);
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert!(!vm.is_stopped());
    assert_eq!(report.from_schema_version, 1);
    assert_eq!(report.to_schema_version, 1);
}

#[test]
fn unchanged_schema_loads_under_strict_without_reports() {
    let reg = registry();
    let old = compile_ok(OLD);
    let (bytes, _) = saved_old(&reg, &old);
    let (vm, report) = Vm::load(&old, &reg, VmConfig::default(), &bytes).unwrap();
    assert!(vm.is_stopped());
    assert!(report.type_changed_vars.is_empty() && report.changed_types.is_empty());
    assert!(report.migrated_vars.is_empty() && report.migrate_failed_vars.is_empty());
}

#[test]
fn added_and_removed_variables_are_not_schema_mismatches() {
    let reg = registry();
    let old = compile_ok(OLD);
    let new = compile_ok(
        "struct P { hp: int }\nsave var p: P = P { hp: 5 }\nsave var extra: int = 9\npersistent var best: int = 0\nlabel start {\n  one #a\n  two\n}\n",
    );
    let (bytes, _) = saved_old(&reg, &old);
    let (_, report) = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.added_vars, ["main.extra"]);
    assert_eq!(report.removed_vars, ["main.n", "main.s"]);
}

#[test]
fn strict_load_persistent_fails_and_keeps_the_vm_and_lenient_resets() {
    let reg = registry();
    let old = compile_ok(OLD);
    let new = compile_ok(NEW);
    let (_, persistent) = saved_old(&reg, &old);
    let mut vm = Vm::new(&new, &reg, VmConfig::default()).unwrap();
    let e = vm.load_persistent(&persistent).unwrap_err();
    let LoadErrorKind::SchemaMismatch(diffs) = e.kind else {
        panic!("{e:?}");
    };
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].name, "main.best");
    assert_eq!(var(&vm, "best").to_text(), "-");
    let mut vm = Vm::new(
        &new,
        &reg,
        VmConfig::default().load_policy(LoadPolicy::Lenient),
    )
    .unwrap();
    let report = vm.load_persistent(&persistent).unwrap();
    assert_eq!(report.type_changed_vars, ["main.best"]);
    assert_eq!(var(&vm, "best").to_text(), "-");
}

#[test]
fn schema_versions_outside_the_supported_range_fail() {
    let prog = compile_ok(OLD);
    assert!(crate::vm::save::check_schema_version(&prog, 1).is_ok());
    assert_eq!(
        crate::vm::save::check_schema_version(&prog, 2)
            .unwrap_err()
            .kind,
        LoadErrorKind::SaveFromNewerVersion {
            saved: 2,
            current: 1
        }
    );
    assert_eq!(
        crate::vm::save::check_schema_version(&prog, 0)
            .unwrap_err()
            .kind,
        LoadErrorKind::SaveVersionUnsupported { saved: 0, min: 1 }
    );
}
