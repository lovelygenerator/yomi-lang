//! Registration

use super::*;

#[test]
fn registering_package_exposes_command_kinds_variables_and_dependencies() {
    let reg = full_registry();
    assert_eq!(reg.command_kind("counter.inc"), Some(CommandKind::Effect));
    assert_eq!(reg.command_kind("counter.get"), Some(CommandKind::Query));
    assert_eq!(reg.command_kind("counter.wait"), Some(CommandKind::Stop));
    assert_eq!(reg.command_kind("ui.say"), Some(CommandKind::Stop));
    assert_eq!(reg.command_kind("counter.nothing"), None);
    let names: Vec<String> = reg.packages().map(|p| p.name).collect();
    assert_eq!(names, ["counter", "other", "mirror"]);
    let mirror = reg.packages().find(|p| p.name == "mirror").unwrap();
    assert_eq!(mirror.depends, ["counter"]);
    assert_eq!(mirror.version, 1);
    let vars: Vec<(&str, Kind, bool)> = reg
        .vars()
        .iter()
        .map(|v| (v.name.as_str(), v.kind, v.read_only))
        .collect();
    assert_eq!(
        vars,
        [
            ("counter.step", Kind::Save, false),
            ("counter.best", Kind::Persistent, false),
            ("counter.locked", Kind::Save, true),
            ("counter.title", Kind::Save, false),
        ]
    );
}

/// A package that registers a command whose name's first half differs from `NAME`.
pub(super) struct Stray;

impl Package for Stray {
    const NAME: &'static str = "stray";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("stray.ok", |_: &mut Ctx<Stray>| {});
        reg.effect("elsewhere.bad", |_: &mut Ctx<Stray>| {});
    }
}

#[test]
fn commands_whose_prefix_differs_from_package_name_are_rejected_and_registry_kept() {
    let mut reg = host_registry();
    assert_eq!(
        reg.package::<Stray>(),
        Err(RegistryError::InvalidName("elsewhere.bad".to_string()))
    );
    // `stray.ok`, registered partway, does not remain either
    assert_eq!(reg.packages().count(), 0);
    assert_eq!(reg.command_kind("stray.ok"), None);
    reg.package::<Counter>().unwrap();
}

pub(super) struct Twice;

impl Package for Twice {
    const NAME: &'static str = "twice";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.effect("twice.a", |_: &mut Ctx<Twice>| {});
        reg.effect("twice.a", |_: &mut Ctx<Twice>| {});
    }
}

pub(super) struct QueryNothing;

impl Package for QueryNothing {
    const NAME: &'static str = "qn";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.query("qn.nothing", |_: &Ctx<QueryNothing>| {});
    }
}

pub(super) struct BadVar;

impl Package for BadVar {
    const NAME: &'static str = "badvar";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.var::<Vec<()>>("badvar.items", Kind::Save, vec![]);
    }
}

pub(super) struct BadVarName;

impl Package for BadVarName {
    const NAME: &'static str = "badname";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.var::<i64>("other.x", Kind::Save, 0).read_only();
    }
}

#[test]
fn duplicate_names_valueless_queries_and_unusable_variables_are_rejected() {
    let mut reg = Registry::new();
    assert_eq!(
        reg.package::<Twice>(),
        Err(RegistryError::DuplicateCommand("twice.a".to_string()))
    );
    assert_eq!(
        reg.package::<QueryNothing>(),
        Err(RegistryError::QueryWithoutValue("qn.nothing".to_string()))
    );
    assert_eq!(
        reg.package::<BadVar>(),
        Err(RegistryError::UnsupportedVarType(
            "badvar.items".to_string()
        ))
    );
    assert_eq!(
        reg.package::<BadVarName>(),
        Err(RegistryError::InvalidName("other.x".to_string()))
    );
    reg.package::<Counter>().unwrap();
    assert_eq!(
        reg.package::<Counter>(),
        Err(RegistryError::DuplicatePackage("counter".to_string()))
    );
}

#[test]
fn command_names_cannot_clash_with_stop_commands_or_variables() {
    let mut reg = Registry::new();
    reg.stop::<(), ()>("counter.get").unwrap();
    // Collides with the name of a stop command
    assert_eq!(
        reg.package::<Counter>(),
        Err(RegistryError::DuplicateCommand("counter.get".to_string()))
    );
}

pub(super) struct SelfDependent;

impl Package for SelfDependent {
    const NAME: &'static str = "selfdep";
    const DEPENDS: &'static [&'static str] = &["selfdep"];
    type State = ();
    type Persistent = ();

    fn register(_reg: &mut PackageReg<'_, Self>) {}
}

pub(super) struct DepA;
pub(super) struct DepB;

impl Package for DepA {
    const NAME: &'static str = "depa";
    const DEPENDS: &'static [&'static str] = &["depb"];
    type State = ();
    type Persistent = ();

    fn register(_reg: &mut PackageReg<'_, Self>) {}
}

impl Package for DepB {
    const NAME: &'static str = "depb";
    const DEPENDS: &'static [&'static str] = &["depa"];
    type State = ();
    type Persistent = ();

    fn register(_reg: &mut PackageReg<'_, Self>) {}
}

#[test]
fn unregistered_and_cyclic_dependencies_are_rejected_at_registration() {
    let mut reg = Registry::new();
    // The dependency has not been registered first
    assert_eq!(
        reg.package::<Mirror>(),
        Err(RegistryError::MissingDependency {
            package: "mirror".to_string(),
            dependency: "counter".to_string()
        })
    );
    // A dependency on itself is a cycle
    assert_eq!(
        reg.package::<SelfDependent>(),
        Err(RegistryError::DependencyCycle("selfdep".to_string()))
    );
    // Mutual dependencies give an unregistered dependency whichever is registered first (cyclic dependencies cannot be
    // registered)
    assert!(matches!(
        reg.package::<DepA>(),
        Err(RegistryError::MissingDependency { .. })
    ));
    assert!(matches!(
        reg.package::<DepB>(),
        Err(RegistryError::MissingDependency { .. })
    ));
    assert_eq!(reg.packages().count(), 0);
    // It passes if the dependency is registered first
    reg.package::<Counter>().unwrap();
    reg.package::<Mirror>().unwrap();
}

#[test]
fn host_declared_variables_are_used_by_importing_namespace() {
    let mut reg = host_registry();
    reg.var::<i64>("ui.text_speed", Kind::Persistent, 3)
        .unwrap();
    reg.var::<bool>("ui.debug", Kind::Save, false)
        .unwrap()
        .read_only();
    // Invalid name forms, duplicates, unusable types, unusable values
    assert!(matches!(
        reg.var::<i64>("speed", Kind::Save, 0),
        Err(RegistryError::InvalidName(_))
    ));
    assert!(matches!(
        reg.var::<i64>("ui.text_speed", Kind::Save, 0),
        Err(RegistryError::DuplicateCommand(_))
    ));
    assert!(matches!(
        reg.var::<Vec<()>>("ui.names", Kind::Save, vec![]),
        Err(RegistryError::UnsupportedVarType(_))
    ));
    assert!(matches!(
        reg.var::<f64>("ui.ratio", Kind::Save, f64::NAN),
        Err(RegistryError::InvalidVarValue(_))
    ));
    let prog = compile_ok(
        "import ui\nlabel start {\n  ui.text_speed += 1\n  速さ ${ui.text_speed} #a\n}\n",
        &reg,
    );
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    to_stop(&mut vm);
    assert_eq!(vm.get_var("ui.text_speed"), Some(Value::Int(4)));
    // No `import`, or assignment to read_only, are compile errors
    assert!(compile("label start {\n  ui.text_speed = 1\n}\n", "main", &reg).is_err());
    assert!(
        compile(
            "import ui\nlabel start {\n  ui.debug = true\n}\n",
            "main",
            &reg
        )
        .is_err()
    );
}

thread_local! {
    /// Operations that need something to attach to, tried in `AttachAfterVar`'s `register`.
    static ATTACH: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

/// A package that, after a variable, attaches things that can only be attached to commands (aliases, parameter names,
/// defaults, translatable bodies).
pub(super) struct AttachAfterVar;

impl Package for AttachAfterVar {
    const NAME: &'static str = "attach";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.var::<i64>("attach.v", Kind::Save, 0);
        match ATTACH.get() {
            0 => reg.alias("attach.old"),
            1 => reg.param_names(&["x"]),
            2 => reg.defaults(&[("x", Value::Int(1))]),
            _ => reg.text_param(0),
        };
        // Only the first error remains
        reg.effect("attach.v", |_: &mut Ctx<AttachAfterVar>| {});
    }
}

/// A package that adds a description before registering anything.
pub(super) struct DocFirst;

impl Package for DocFirst {
    const NAME: &'static str = "docfirst";
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.doc("説明");
    }
}

#[test]
fn command_only_attachments_are_rejected_unless_previous_is_command() {
    for action in 0..4 {
        ATTACH.set(action);
        let mut reg = Registry::new();
        assert_eq!(
            reg.package::<AttachAfterVar>(),
            Err(RegistryError::NoCommand),
            "操作 {action}"
        );
        assert_eq!(reg.packages().count(), 0);
    }
    let mut reg = Registry::new();
    assert_eq!(reg.package::<DocFirst>(), Err(RegistryError::NoCommand));
}
