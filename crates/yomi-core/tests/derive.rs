//! Tests of registry structs and enums (`#[derive(YomiType)]`), host variables of compound types, and aliases.

use yomi_core::{
    Ctx, FromValue, Input, IntoValue, Kind, LoadErrorKind, Package, PackageReg, Program, Registry,
    RegistryError, Snapshot, Sugar, Ty, Value, Vm, VmConfig, VmErrorKind, Yield, YomiDecl, YomiMap,
    YomiType, compile, compile_with_warnings,
};

#[derive(Debug, Clone, PartialEq, YomiType)]
#[yomi(namespace = "game")]
struct Chara {
    name: String,
    hp: i64,
    tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, YomiType)]
#[yomi(namespace = "game")]
enum Mood {
    Calm,
    Angry(i64),
    Mixed(String, Option<i64>),
}

#[derive(Debug, Clone, PartialEq, YomiType)]
#[yomi(namespace = "game", name = "Party")]
struct Team {
    leader: Chara,
    mood: Mood,
    scores: YomiMap<String, i64>,
}

fn reg() -> Registry {
    let mut r = Registry::new();
    r.ty::<Chara>().unwrap();
    r.ty::<Mood>().unwrap();
    r.ty::<Team>().unwrap();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.stop::<(Chara,), ()>("game.show").unwrap();
    r.stop::<(Mood,), Mood>("game.feel").unwrap();
    r.stop::<(Team,), Option<Chara>>("game.team").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.ignore(Sugar::Quote).unwrap();
    r.ignore(Sugar::Choice).unwrap();
    r
}

fn program(src: &str, r: &Registry) -> Program {
    compile(src, "main", r).unwrap_or_else(|d| panic!("{d:?}"))
}

fn chara() -> Chara {
    Chara {
        name: "案内人".into(),
        hp: 3,
        tags: vec!["a".into()],
    }
}

#[test]
fn derived_type_has_same_declaration_and_values_as_manual_impl() {
    assert_eq!(Chara::ty(), Ty::Struct("game.Chara".into()));
    assert_eq!(Team::ty(), Ty::Struct("game.Party".into()));
    let decl = Chara::decl();
    assert_eq!(decl.name, "game.Chara");
    assert_eq!(
        decl.signature(),
        "struct { name: str, hp: int, tags: list<str> }"
    );
    assert_eq!(
        Mood::decl().signature(),
        "enum { Calm, Angry(int), Mixed(str, Option<int>) }"
    );
    let v = chara().into_value();
    assert_eq!(
        v,
        Value::Struct(std::sync::Arc::new(yomi_core::StructValue {
            ty: "game.Chara".into(),
            fields: vec![
                ("name".into(), Value::Str("案内人".into())),
                ("hp".into(), Value::Int(3)),
                (
                    "tags".into(),
                    Value::List(vec![Value::Str("a".into())].into())
                ),
            ]
        }))
    );
    assert_eq!(Chara::from_value(&v), Some(chara()));
    for m in [Mood::Calm, Mood::Angry(2), Mood::Mixed("x".into(), None)] {
        assert_eq!(Mood::from_value(&m.clone().into_value()), Some(m));
    }
    // Values whose type name, variant or count does not match cannot be read
    assert_eq!(Mood::from_value(&chara().into_value()), None);
    assert_eq!(
        Mood::from_value(&Value::Enum(std::sync::Arc::new(yomi_core::EnumValue {
            ty: "game.Mood".into(),
            variant: "Angry".into(),
            args: vec![],
        }))),
        None
    );
}

#[test]
fn registered_types_work_in_command_args_returns_struct_literals_and_match() {
    let r = reg();
    let p = program(
        r#"
import game
label start {
  let c = game.Chara { name: "案内人", hp: 3, tags: ["a"] }
  @game.show(c)
  let m = @game.feel(game.Mood.Angry(2))
  match m {
    Calm => 穏やか
    Angry(n) => 怒り ${n}
    Mixed(s, o) => 混ぜ
  }
  ${m}
}
"#,
        &r,
    );
    let mut vm = Vm::new(&p, &r, VmConfig::default()).unwrap();
    let Yield::Stop { args, .. } = vm.run().unwrap() else {
        panic!()
    };
    assert_eq!(Chara::from_value(&args[0]), Some(chara()));
    vm.resume(Input::Return(Value::Unit)).unwrap();
    let Yield::Stop { args, .. } = vm.run().unwrap() else {
        panic!()
    };
    assert_eq!(Mood::from_value(&args[0]), Some(Mood::Angry(2)));
    let ret = Mood::Mixed("x".into(), Some(1)).into_value();
    vm.resume(Input::Return(ret)).unwrap();
    let Yield::Stop { args, .. } = vm.run().unwrap() else {
        panic!()
    };
    // The "mixed" row
    assert_eq!(args, vec![Value::Str("混ぜ".into())]);
}

#[test]
fn registered_types_work_in_script_annotations_and_type_diagnostics() {
    let r = reg();
    let e = compile(
        "import game\nlabel start {\n  let c: game.Chara = game.Chara { name: \"a\", hp: 1 }\n}\n",
        "main",
        &r,
    )
    .unwrap_err();
    assert_eq!(e[0].code.as_str(), "missing_field");
    let e = compile(
        "import game\nlabel start {\n  let c = game.Nope { }\n}\n",
        "main",
        &r,
    )
    .unwrap_err();
    assert_eq!(e[0].code.as_str(), "unknown_type");
    // Cannot be used without `import`ing the namespace
    let e = compile("label start {\n  let m = game.Mood.Calm\n}\n", "main", &r).unwrap_err();
    assert!(!e.is_empty());
}

#[test]
fn script_declared_types_do_not_clash_with_registry_types_thanks_to_module_names() {
    let r = reg();
    compile(
        "import game\nstruct Chara { x: int }\nlabel start {\n  let a = Chara { x: 1 }\n  let b = game.Chara { name: \"\", hp: 0, tags: [] }\n}\n",
        "main",
        &r,
    )
    .unwrap_or_else(|d| panic!("{d:?}"));
    // A module with the same name as the namespace cannot be made
    let e = compile("label start {\n}\n", "game", &r).unwrap_err();
    assert_eq!(e[0].code.as_str(), "duplicate_module");
}

#[test]
fn type_registration_errors_are_registration_errors() {
    #[derive(YomiType)]
    #[yomi(namespace = "x")]
    struct UsesChara {
        c: Chara,
    }
    #[derive(YomiType)]
    #[yomi(namespace = "x")]
    struct SelfRef {
        kids: Vec<SelfRef>,
    }
    #[derive(YomiType)]
    #[yomi(namespace = "x")]
    struct FloatKey {
        m: YomiMap<f64, i64>,
    }
    #[derive(YomiType)]
    #[yomi(namespace = "x")]
    struct Empty {}
    let mut r = Registry::new();
    assert!(matches!(
        r.ty::<UsesChara>(),
        Err(RegistryError::UnknownType { .. })
    ));
    assert!(matches!(
        r.ty::<SelfRef>(),
        Err(RegistryError::RecursiveType(_))
    ));
    assert!(matches!(
        r.ty::<FloatKey>(),
        Err(RegistryError::InvalidMapKey(_))
    ));
    r.ty::<Empty>().unwrap();
    assert!(matches!(
        r.ty::<Empty>(),
        Err(RegistryError::DuplicateType(_))
    ));
    r.ty::<Chara>().unwrap();
    r.ty::<UsesChara>().unwrap();
    // Variables and commands with the same name as a type name cannot be made
    assert!(matches!(
        r.stop::<(), ()>("game.Chara"),
        Err(RegistryError::DuplicateCommand(_))
    ));
    // A command that uses an unregistered type
    let mut r2 = Registry::new();
    assert!(matches!(
        r2.stop::<(Chara,), ()>("game.show"),
        Err(RegistryError::UnknownType { .. })
    ));
    // A parameter of a type that cannot be a `map` key
    assert!(matches!(
        r2.stop::<(YomiMap<f64, i64>,), ()>("game.m"),
        Err(RegistryError::InvalidMapKey(_))
    ));
}

/// Registers types, commands and variables in a package.
struct Cast;

impl Package for Cast {
    const NAME: &'static str = "game";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.ty::<Chara>().doc("登場人物");
        reg.ty::<Mood>();
        reg.query("game.hp_of", |_: &Ctx<Cast>, c: Chara| -> i64 { c.hp })
            .doc("体力を返す")
            .param_names(&["chara"]);
        reg.effect("game.noop", |_: &mut Ctx<Cast>, _m: Mood| {})
            .alias("game.nothing")
            .alias("old.noop");
        reg.var::<Chara>("game.leader", Kind::Save, chara());
        reg.var::<Vec<Chara>>("game.party", Kind::Persistent, vec![]);
        reg.var::<Option<Mood>>("game.mood", Kind::Save, None)
            .read_only();
        reg.var::<YomiMap<String, i64>>("game.flags", Kind::Save, YomiMap(vec![]));
    }
}

fn pkg_reg() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    r.bind(Sugar::Plain, "ui.say").unwrap();
    r.ignore(Sugar::Quote).unwrap();
    r.ignore(Sugar::Choice).unwrap();
    r.package::<Cast>().unwrap();
    r
}

#[test]
fn types_registered_in_package_must_use_package_namespace() {
    struct Wrong;
    impl Package for Wrong {
        const NAME: &'static str = "other";
        type State = ();
        type Persistent = ();
        fn register(reg: &mut PackageReg<'_, Self>) {
            reg.ty::<Chara>();
        }
    }
    let mut r = Registry::new();
    assert!(matches!(
        r.package::<Wrong>(),
        Err(RegistryError::InvalidName(_))
    ));
    assert!(r.types().is_empty());
    let r = pkg_reg();
    assert_eq!(r.types().len(), 2);
    assert_eq!(r.type_doc("game.Chara"), Some("登場人物"));
}

#[test]
fn compound_host_variables_readable_and_writable_from_script_and_get_set_var_work() {
    let r = pkg_reg();
    let p = program(
        r#"
import game
label start {
  game.leader.hp += 2
  game.party.push(game.leader)
  game.flags["a"] = 1
  let h = game.hp_of(game.leader)
  ${h}
  @ui.say("x")
}
"#,
        &r,
    );
    let mut vm = Vm::new(&p, &r, VmConfig::default()).unwrap();
    let Yield::Stop { args, .. } = vm.run().unwrap() else {
        panic!()
    };
    let _ = args;
    assert_eq!(
        Chara::from_value(&vm.get_var("game.leader").unwrap())
            .unwrap()
            .hp,
        5
    );
    assert_eq!(
        vm.get_var("game.party").unwrap(),
        Value::List(vec![Chara { hp: 5, ..chara() }.into_value()].into())
    );
    assert_eq!(
        vm.get_var("game.flags").unwrap(),
        Value::Map(vec![(Value::Str("a".into()), Value::Int(1))].into())
    );
    // The host can write even read_only variables. Values of the wrong type are rejected
    vm.set_var("game.mood", Some(Mood::Calm).into_value())
        .unwrap();
    assert!(vm.set_var("game.mood", Value::Int(1)).is_err());
    // Structs whose fields do not match in type are also rejected
    let bad = Value::Struct(std::sync::Arc::new(yomi_core::StructValue {
        ty: "game.Chara".into(),
        fields: vec![("name".into(), Value::Int(1))],
    }));
    assert!(vm.set_var("game.leader", bad).is_err());
    assert_eq!(
        vm.set_var(
            "game.flags",
            Value::Map(vec![(Value::Str("k".into()), Value::Float(f64::NAN))].into())
        )
        .unwrap_err()
        .kind,
        VmErrorKind::NonFiniteFloat
    );
    // Assignment to read_only is a compile error
    let e = compile(
        "import game\nlabel start {\n  game.mood = None\n}\n",
        "main",
        &r,
    )
    .unwrap_err();
    assert_eq!(e[0].code.as_str(), "assign_to_read_only");
}

#[test]
fn compound_host_variables_restore_with_save_load_and_rollback() {
    let r = pkg_reg();
    let p = program(
        r#"
import game
label start {
  @ui.say("a")
  game.leader.hp = 100
  game.party.push(game.leader)
  @ui.say("b")
}
"#,
        &r,
    );
    let mut vm = Vm::new(&p, &r, VmConfig::default()).unwrap();
    vm.run().unwrap();
    let snap: Snapshot = vm.snapshot().unwrap();
    let bytes = vm.save().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    assert_eq!(
        Chara::from_value(&vm.get_var("game.leader").unwrap())
            .unwrap()
            .hp,
        100
    );
    let persistent = vm.save_persistent();
    // Rollback: Save variables go back, Persistent ones do not
    vm.restore(&snap).unwrap();
    assert_eq!(
        Chara::from_value(&vm.get_var("game.leader").unwrap())
            .unwrap()
            .hp,
        3
    );
    assert_eq!(
        vm.get_var("game.party").unwrap().to_text(),
        "[Chara { name: 案内人, hp: 100, tags: [a] }]"
    );
    // Save → load
    let (vm2, report) = Vm::load(&p, &r, VmConfig::default(), &bytes).unwrap();
    assert!(report.changed_types.is_empty());
    assert_eq!(
        Chara::from_value(&vm2.get_var("game.leader").unwrap())
            .unwrap()
            .hp,
        3
    );
    // Persistent data
    let mut vm3 = Vm::new(&p, &r, VmConfig::default()).unwrap();
    vm3.load_persistent(&persistent).unwrap();
    assert_eq!(
        vm3.get_var("game.party").unwrap(),
        Value::List(vec![Chara { hp: 100, ..chara() }.into_value()].into())
    );
}

#[test]
fn changed_type_declaration_resets_host_variables_of_that_type_and_reports_changed_types() {
    let r = pkg_reg();
    let p = program(
        "import game\nlabel start {\n  @ui.say(\"a\")\n  game.leader.hp = 9\n  @ui.say(\"b\")\n}\n",
        &r,
    );
    let mut vm = Vm::new(&p, &r, VmConfig::default()).unwrap();
    vm.run().unwrap();
    vm.resume(Input::Return(Value::Unit)).unwrap();
    vm.run().unwrap();
    let bytes = vm.save().unwrap();
    // A registry where `Chara` has an added field
    #[derive(YomiType)]
    #[yomi(namespace = "game", name = "Chara")]
    struct Chara2 {
        name: String,
        hp: i64,
        tags: Vec<String>,
        level: i64,
    }
    struct Cast2;
    impl Package for Cast2 {
        const NAME: &'static str = "game";
        type State = ();
        type Persistent = ();
        fn register(reg: &mut PackageReg<'_, Self>) {
            reg.ty::<Chara2>();
            reg.var::<Chara2>(
                "game.leader",
                Kind::Save,
                Chara2 {
                    name: "新".into(),
                    hp: 1,
                    tags: vec![],
                    level: 1,
                },
            );
        }
    }
    let mut r2 = Registry::new();
    r2.stop::<(String,), ()>("ui.say").unwrap();
    r2.bind(Sugar::Plain, "ui.say").unwrap();
    r2.ignore(Sugar::Quote).unwrap();
    r2.ignore(Sugar::Choice).unwrap();
    r2.package::<Cast2>().unwrap();
    let p2 = program(
        "import game\nlabel start {\n  @ui.say(\"a\")\n  @ui.say(\"b\")\n}\n",
        &r2,
    );
    let (vm2, report) = Vm::load(&p2, &r2, VmConfig::default(), &bytes).unwrap();
    assert_eq!(report.changed_types, ["game.Chara"]);
    assert_eq!(report.type_changed_vars, ["game.leader"]);
    let v = vm2.get_var("game.leader").unwrap();
    assert!(matches!(&v, Value::Struct(st) if st.fields.len() == 4));
    let _ = LoadErrorKind::Corrupt;
}

#[test]
fn command_aliases_can_be_called_by_old_name_with_warning() {
    let r = pkg_reg();
    assert_eq!(r.aliases().count(), 2);
    for old in ["game.nothing", "old.noop"] {
        let import = if old.starts_with("old") {
            "import old\n"
        } else {
            "import game\n"
        };
        let src = format!("{import}label start {{\n  @{old}(game.Mood.Calm)\n}}\n");
        // `old` can be `import old`ed, and an `import` of the old name's namespace is required
        let src = if old.starts_with("old") {
            format!("import old\nimport game\nlabel start {{\n  @{old}(game.Mood.Calm)\n}}\n")
        } else {
            src
        };
        let (p, warnings) =
            compile_with_warnings(&src, "main", &r).unwrap_or_else(|d| panic!("{old}: {d:?}"));
        assert_eq!(warnings.len(), 1, "{old}");
        assert_eq!(warnings[0].code.as_str(), "deprecated_command_name");
        assert_eq!(warnings[0].arg_str("name"), Some(old));
        assert_eq!(warnings[0].arg_str("new"), Some("game.noop"));
        // It can run (the command with the official name is called)
        let mut vm = Vm::new(&p, &r, VmConfig::default()).unwrap();
        assert!(matches!(vm.run().unwrap(), Yield::End));
    }
    // An error if the old name's namespace is not imported
    let e = compile(
        "import game\nlabel start {\n  @old.noop(game.Mood.Calm)\n}\n",
        "main",
        &r,
    )
    .unwrap_err();
    assert!(e.iter().any(|d| d.code.as_str() == "module_not_imported"));
}

#[test]
fn alias_collisions_are_registration_errors() {
    let mut r = Registry::new();
    r.stop::<(), ()>("a.one").unwrap();
    r.stop::<(), ()>("a.two").unwrap();
    r.var::<i64>("a.v", Kind::Save, 0).unwrap();
    assert!(matches!(
        r.alias("a.two", "a.one"),
        Err(RegistryError::DuplicateCommand(_))
    ));
    assert!(matches!(
        r.alias("a.v", "a.one"),
        Err(RegistryError::DuplicateCommand(_))
    ));
    r.alias("a.old", "a.one").unwrap();
    assert!(matches!(
        r.alias("a.old", "a.two"),
        Err(RegistryError::DuplicateCommand(_))
    ));
    assert!(matches!(
        r.alias("a.x", "a.none"),
        Err(RegistryError::UnknownCommand(_))
    ));
    assert!(matches!(
        r.alias("a", "a.one"),
        Err(RegistryError::InvalidName(_))
    ));
    // An alias points only to an official name, not to another alias
    assert!(matches!(
        r.alias("a.y", "a.old"),
        Err(RegistryError::UnknownCommand(_))
    ));
    // A command with the same name as an alias cannot be made
    assert!(matches!(
        r.stop::<(), ()>("a.old"),
        Err(RegistryError::DuplicateCommand(_))
    ));
    // An alias in a package when there is no previous command
    struct NoCmd;
    impl Package for NoCmd {
        const NAME: &'static str = "z";
        type State = ();
        type Persistent = ();
        fn register(reg: &mut PackageReg<'_, Self>) {
            reg.alias("z.old");
        }
    }
    assert!(matches!(
        r.package::<NoCmd>(),
        Err(RegistryError::NoCommand)
    ));
}

#[test]
fn registry_from_declaration_file_can_check_but_not_run() {
    let r = pkg_reg();
    let json = r.export_decls();
    let d = Registry::from_decls(&json).unwrap();
    assert!(d.is_decl_only());
    let src =
        "import game\nlabel start {\n  @game.nothing(game.Mood.Calm)\n  game.leader.hp += 1\n}\n";
    let a = compile_with_warnings(src, "main", &r).map(|(_, w)| w);
    let b = compile_with_warnings(src, "main", &d);
    let b_warn = b.as_ref().map(|(_, w)| w.clone()).map_err(|e| e.clone());
    assert_eq!(a, b_warn);
    let (p, _) = b.unwrap();
    assert_eq!(
        Vm::new(&p, &d, VmConfig::default()).err().unwrap().kind,
        VmErrorKind::DeclOnlyRegistry
    );
    assert_eq!(d.export_decls(), json);
    // Type diagnostics are the same too
    for bad in [
        "import game\nlabel start {\n  let c = game.Chara { name: \"a\" }\n}\n",
        "import game\nlabel start {\n  @game.hp_of(1)\n}\n",
        "import game\nlabel start {\n  game.mood = None\n}\n",
        "label start {\n  game.leader.hp = 1\n}\n",
    ] {
        let x = compile_with_warnings(bad, "main", &r).map(|(_, w)| w);
        let y = compile_with_warnings(bad, "main", &d).map(|(_, w)| w);
        assert!(x.is_err());
        assert_eq!(x, y, "{bad}");
    }
}
