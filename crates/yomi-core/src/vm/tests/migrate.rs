//! Tests of applying `migrate` blocks when loading saves and persistent data.

use super::util::{self, *};
use crate::compile::{CompileOptions, compile_modules_with_options};
use crate::program::Program;
use crate::registry::Sugar;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[Sugar::Quote], |_| {})
}

fn fnv(body: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn frozen(version: u32, body: &str) -> String {
    format!("schema {version} hash \"{}\"\n{body}", fnv(body))
}

const BODY: &str = "label start {\n  one #a\n  two\n}\n";

const V1: &str = "struct Item { id: str }\nsave var hp: int = 7\nsave var gold: int = 3\nsave var flag: bool = true\nsave var bag: list<Item> = [Item { id: \"a\" }]\npersistent var best: int = 4\n";

const V1_SCHEMA: &str = "\nmodule main {\n    struct Item { id: str }\n    save var bag: list<main.Item>\n    save var flag: bool\n    save var gold: int\n    save var hp: int\n    persistent var best: int\n}\n";

const V2: &str = "struct Item { id: str, count: int }\nsave var hp: float = 0.0\nsave var coins: int = 0\nsave var bag: list<Item> = []\npersistent var best: float = 0.0\n";

const MIGRATE_V2: &str = "migrate 2 {
    convert v1::main.hp: int -> main.hp: float = |v| to_float(v) * 2.0
    rename v1::main.gold -> main.coins
    drop v1::main.flag
    convert v1::main.bag: list<v1::main.Item> -> list<main.Item> =
        |xs| xs.map(|x| main.Item { id: x.id, count: 1 })
    convert v1::main.best: int -> main.best: float = |v| to_float(v) + 0.5
}
";

fn v1() -> Program {
    util::compile_ok(&format!("{V1}{BODY}"), &registry())
}

fn migrating(migrate: &str, new_src: &str) -> Program {
    let opts = CompileOptions::default()
        .migrations(migrate)
        .frozen_schema_file("schema/v1.yomi", &frozen(1, V1_SCHEMA));
    match compile_modules_with_options(&[("main", &format!("{new_src}{BODY}"))], &registry(), &opts)
    {
        Ok((p, _)) => p,
        Err(d) => panic!("コンパイルできない: {d:?}"),
    }
}

fn saved_v1() -> (Vec<u8>, Vec<u8>) {
    let reg = registry();
    let prog = v1();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    (vm.save().unwrap(), vm.save_persistent())
}

#[test]
fn old_save_is_migrated_by_convert_rename_and_drop() {
    let reg = registry();
    let new = migrating(MIGRATE_V2, V2);
    let (bytes, _) = saved_v1();
    let (vm, report) = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.from_schema_version, 1);
    assert_eq!(report.to_schema_version, 2);
    assert_eq!(report.migrated_vars, ["main.bag", "main.coins", "main.hp"]);
    assert!(report.migrate_failed_vars.is_empty());
    assert_eq!(var(&vm, "hp"), Value::Float(14.0));
    assert_eq!(var(&vm, "coins"), Value::Int(3));
    let bag = var(&vm, "bag");
    let Value::List(items) = &bag else {
        panic!("{bag:?}");
    };
    assert_eq!(items.len(), 1);
    assert_eq!(report.removed_vars, Vec::<String>::new());
    assert!(vm.is_stopped());
}

#[test]
fn old_persistent_data_is_migrated_the_same_way() {
    let reg = registry();
    let new = migrating(MIGRATE_V2, V2);
    let (_, persistent) = saved_v1();
    let mut vm = Vm::new(&new, &reg, VmConfig::default()).unwrap();
    let report = vm.load_persistent(&persistent).unwrap();
    assert_eq!(report.migrated_vars, ["main.best"]);
    assert_eq!(var(&vm, "best"), Value::Float(4.5));
}

#[test]
fn unhandled_type_change_is_a_schema_mismatch_under_strict() {
    let reg = registry();
    // `hp` changes from int to float but no `migrate` handles it
    let new = migrating("migrate 2 {\n}\n", "save var hp: float = 0.0\n");
    let (bytes, _) = saved_v1();
    let e = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap_err();
    let LoadErrorKind::SchemaMismatch(diffs) = e.kind else {
        panic!("{e:?}");
    };
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].name, "main.hp");
    let cfg = VmConfig::default().load_policy(LoadPolicy::Lenient);
    let (vm, report) = Vm::load(&new, &reg, cfg, &bytes).unwrap();
    assert_eq!(report.type_changed_vars, ["main.hp"]);
    assert_eq!(var(&vm, "hp"), Value::Float(0.0));
}

#[test]
fn convert_failing_at_run_time_is_an_error_under_strict_and_initial_value_under_lenient() {
    let reg = registry();
    let new = migrating(
        "migrate 2 {\n    convert v1::main.hp: int -> main.hp: float = |v| to_float(10 / (v - 7))\n}\n",
        "save var hp: float = -1.0\n",
    );
    let (bytes, _) = saved_v1();
    let e = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap_err();
    assert_eq!(
        e.kind,
        LoadErrorKind::MigrateFailed(vec!["main.hp".to_string()])
    );
    let cfg = VmConfig::default().load_policy(LoadPolicy::Lenient);
    let (vm, report) = Vm::load(&new, &reg, cfg, &bytes).unwrap();
    assert_eq!(report.migrate_failed_vars, ["main.hp"]);
    assert!(report.added_vars.is_empty());
    assert_eq!(report.resolved_by, ResolvedBy::LabelStart);
    assert_eq!(var(&vm, "hp"), Value::Float(-1.0));
}

#[test]
fn saves_older_than_the_supported_range_are_rejected() {
    let reg = registry();
    let opts = CompileOptions::default()
        .migrations("min_save_version 2\nmigrate 3 {\n}\n")
        .frozen_schema_file("schema/v2.yomi", &frozen(2, V1_SCHEMA));
    let (new, _) =
        compile_modules_with_options(&[("main", &format!("{V1}{BODY}"))], &reg, &opts).unwrap();
    let (bytes, _) = saved_v1();
    let e = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap_err();
    assert!(
        matches!(
            e.kind,
            LoadErrorKind::SaveVersionUnsupported { saved: 1, min: 2 }
        ),
        "{e:?}"
    );
}

#[test]
fn save_is_migrated_across_several_versions_in_order() {
    let reg = registry();
    let old = util::compile_ok(&format!("save var hp: int = 7\n{BODY}"), &reg);
    let mut vm = Vm::new(&old, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();

    let migrations = "migrate 2 {
    convert v1::main.hp: int -> v2::main.hp: float = |v| to_float(v) * 2.0
}
migrate 3 {
    rename v2::main.hp -> main.health
}
";
    let opts = CompileOptions::default()
        .migrations(migrations)
        .frozen_schema_file(
            "schema/v1.yomi",
            &frozen(1, "\nmodule main {\n    save var hp: int\n}\n"),
        )
        .frozen_schema_file(
            "schema/v2.yomi",
            &frozen(2, "\nmodule main {\n    save var hp: float\n}\n"),
        );
    let src = format!("save var health: float = 0.0\n{BODY}");
    let (new, _) = compile_modules_with_options(&[("main", &src)], &reg, &opts).unwrap();
    let (vm, report) = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.from_schema_version, 1);
    assert_eq!(report.to_schema_version, 3);
    assert_eq!(var(&vm, "health"), Value::Float(14.0));
}

#[test]
fn save_made_by_a_released_version_is_migrated_with_the_new_side_frozen() {
    let reg = registry();
    let old = util::compile_ok(&format!("save var hp: int = 7\n{BODY}"), &reg);
    let mut vm = Vm::new(&old, &reg, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();

    let opts = CompileOptions::default()
        .migrations("migrate 2 {\n    convert v1::main.hp: int -> v2::main.hp: float = |v| to_float(v)\n}\n")
        .frozen_schema_file(
            "schema/v1.yomi",
            &frozen(1, "\nmodule main {\n    save var hp: int\n}\n"),
        )
        .frozen_schema_file(
            "schema/v2.yomi",
            &frozen(2, "\nmodule main {\n    save var hp: float\n}\n"),
        );
    let src = format!("save var hp: float = 0.0\n{BODY}");
    let (new, _) = compile_modules_with_options(&[("main", &src)], &reg, &opts).unwrap();
    let (vm, report) = Vm::load(&new, &reg, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.to_schema_version, 2);
    assert_eq!(var(&vm, "hp"), Value::Float(7.0));
}

const V1_SORTED: &str = "\nmodule main {\n    struct Item { id: str }\n    save var bag: list<main.Item>\n    persistent var best: int\n    save var flag: bool\n    save var gold: int\n    save var hp: int\n}\n";

#[test]
fn frozen_schema_matches_the_documented_form() {
    assert_eq!(crate::frozen_schema(&v1()), frozen(1, V1_SORTED));
}

#[test]
fn schema_is_extracted_from_a_save_and_persistent_data() {
    let (save, persistent) = saved_v1();
    let both = crate::extract_schema(Some(&save), Some(&persistent)).unwrap();
    assert_eq!(both, frozen(1, V1_SORTED));
    let only_save = crate::extract_schema(Some(&save), None).unwrap();
    assert!(only_save.contains("save var hp: int") && !only_save.contains("persistent var"));
    assert_eq!(
        crate::extract_schema(None, None),
        Err(crate::ExtractError::NothingGiven)
    );
    let mut broken = save.clone();
    let last = broken.len() - 1;
    broken[last] ^= 0xff;
    assert!(matches!(
        crate::extract_schema(Some(&broken), None),
        Err(crate::ExtractError::Load(_))
    ));
}

#[test]
fn migrate_template_lists_renames_and_conversions_as_comments() {
    let new = util::compile_ok(&format!("{V2}{BODY}"), &registry());
    let got = crate::migrate_template(&new, &frozen(1, V1_SCHEMA), 1)
        .unwrap()
        .unwrap();
    assert!(got.starts_with("migrate 2 {\n"), "{got}");
    assert!(
        got.contains("    // convert v1::main.hp: int -> float = |v| v\n"),
        "{got}"
    );
    assert!(
        got.contains(
            "    // convert v1::main.bag: list<v1::main.Item> -> list<main.Item> = |v| v\n"
        ),
        "{got}"
    );
    assert!(
        got.lines()
            .skip(1)
            .take_while(|l| *l != "}")
            .all(|l| l.trim_start().starts_with("//"))
    );
    let same = v1();
    assert_eq!(
        crate::migrate_template(&same, &frozen(1, V1_SCHEMA), 1).unwrap(),
        None
    );
}
