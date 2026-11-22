//! Structural checks of `migrations` and frozen schema files from the public API.

use yomi_core::{CompileOptions, DiagCode, Registry, Severity, compile_modules_with_options};

fn fnv(body: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in body.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A frozen schema file with a correct hash.
fn frozen(version: u32, body: &str) -> String {
    format!("schema {version} hash \"{}\"\n{body}", fnv(body))
}

const SCRIPT: &str = "save var hp: int = 1\nlabel start {\n  hp = 2\n}\n";
const BODY: &str = "\nmodule main {\n    save var hp: int\n}\n";

fn run(src: &str, opts: &CompileOptions) -> Result<(u32, u32), Vec<DiagCode>> {
    match compile_modules_with_options(&[("main", src)], &Registry::new(), opts) {
        Ok((p, _)) => Ok((p.schema_version(), p.min_save_version())),
        Err(d) => Err(d
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.code)
            .collect()),
    }
}

#[test]
fn no_migrations_means_version_one() {
    assert_eq!(run(SCRIPT, &CompileOptions::default()), Ok((1, 1)));
}

#[test]
fn version_is_the_largest_migrate_and_min_is_read() {
    let opts = CompileOptions::default()
        .migrations("min_save_version 2\nmigrate 3 {\n}\n")
        .frozen_schema_file("schema/v2.yomi", &frozen(2, BODY));
    assert_eq!(run(SCRIPT, &opts), Ok((3, 2)));
}

#[test]
fn migrate_versions_are_checked() {
    let opts = CompileOptions::default().migrations("migrate 1 {\n}\n");
    assert_eq!(
        run(SCRIPT, &opts),
        Err(vec![DiagCode::InvalidMigrateVersion])
    );
    let opts = CompileOptions::default()
        .migrations("migrate 2 {\n}\nmigrate 2 {\n}\n")
        .frozen_schema_file("schema/v1.yomi", &frozen(1, BODY));
    assert_eq!(run(SCRIPT, &opts), Err(vec![DiagCode::DuplicateMigrate]));
}

#[test]
fn min_save_version_is_checked() {
    let opts = CompileOptions::default().migrations("min_save_version 5\n");
    assert_eq!(
        run(SCRIPT, &opts),
        Err(vec![DiagCode::InvalidMinSaveVersion])
    );
    let opts = CompileOptions::default().migrations("min_save_version 0\n");
    assert_eq!(
        run(SCRIPT, &opts),
        Err(vec![DiagCode::InvalidMinSaveVersion])
    );
}

#[test]
fn missing_frozen_schema_and_migrate_are_errors() {
    // version 3, min 1: needs v1 and v2 frozen, migrate 2 and 3
    let opts = CompileOptions::default().migrations("migrate 3 {\n}\n");
    assert_eq!(
        run(SCRIPT, &opts),
        Err(vec![
            DiagCode::MissingFrozenSchema,
            DiagCode::MissingFrozenSchema,
            DiagCode::MissingMigrate,
        ])
    );
}

#[test]
fn frozen_schema_files_are_checked() {
    let bad_name = CompileOptions::default().frozen_schema_file("schema/x.yomi", &frozen(1, BODY));
    assert_eq!(
        run(SCRIPT, &bad_name),
        Err(vec![DiagCode::InvalidSchemaFile])
    );
    let leading_zero =
        CompileOptions::default().frozen_schema_file("schema/v01.yomi", &frozen(1, BODY));
    assert_eq!(
        run(SCRIPT, &leading_zero),
        Err(vec![DiagCode::InvalidSchemaFile])
    );
    let mismatch = CompileOptions::default().frozen_schema_file("schema/v2.yomi", &frozen(3, BODY));
    assert_eq!(
        run(SCRIPT, &mismatch),
        Err(vec![DiagCode::InvalidSchemaFile])
    );
    let no_header = CompileOptions::default().frozen_schema_file("schema/v1.yomi", BODY);
    assert_eq!(
        run(SCRIPT, &no_header),
        Err(vec![DiagCode::InvalidSchemaFile])
    );
}

#[test]
fn frozen_schema_hash_is_verified() {
    let tampered = frozen(1, BODY).replace("save var hp: int", "save var hp: str");
    let opts = CompileOptions::default().frozen_schema_file("schema/v1.yomi", &tampered);
    assert_eq!(run(SCRIPT, &opts), Err(vec![DiagCode::SchemaHashMismatch]));
}

#[test]
fn reserved_module_names_are_rejected() {
    for name in ["schema", "migrations", "v2", "schema::a"] {
        let r = compile_modules_with_options(
            &[(name, SCRIPT)],
            &Registry::new(),
            &CompileOptions::default(),
        );
        let codes: Vec<_> = r.err().unwrap().iter().map(|d| d.code).collect();
        assert!(codes.contains(&DiagCode::InvalidModuleName), "{name}");
    }
    // `v2x` and `version` are ordinary names
    for name in ["v2x", "version", "v"] {
        assert!(
            compile_modules_with_options(
                &[(name, SCRIPT)],
                &Registry::new(),
                &CompileOptions::default()
            )
            .is_ok(),
            "{name}"
        );
    }
}

const SCRIPT2: &str = "save var hp: float = 1.0\nsave var n: int = 0\nlabel start {\n  n = 1\n}\n";
const FROZEN1: &str = "\nmodule main {\n    save var hp: int\n    save var n: int\n}\n";

fn convert(lambda: &str) -> CompileOptions {
    CompileOptions::default()
        .migrations(&format!(
            "migrate 2 {{\n    convert v1::main.hp: int -> main.hp: float = {lambda}\n}}\n"
        ))
        .frozen_schema_file("schema/v1.yomi", &frozen(1, FROZEN1))
}

#[test]
fn convert_expressions_are_compiled_and_type_checked() {
    assert_eq!(run(SCRIPT2, &convert("|v| to_float(v) * 2.0")), Ok((2, 1)));
    assert_eq!(
        run(SCRIPT2, &convert("|v| \"x\"")),
        Err(vec![DiagCode::TypeMismatch])
    );
    let two = run(SCRIPT2, &convert("|a, b| to_float(a)")).unwrap_err();
    assert!(two.contains(&DiagCode::ArgCount), "{two:?}");
}

#[test]
fn convert_expressions_cannot_use_scripts_state() {
    for (lambda, what) in [
        ("|v| to_float(v + main.n)", "variable"),
        ("|v| to_float(v) + random_float()", "random"),
    ] {
        let r =
            compile_modules_with_options(&[("main", SCRIPT2)], &Registry::new(), &convert(lambda));
        let d = r.err().unwrap();
        let hit = d.iter().find(|d| d.code == DiagCode::MigrateImpureExpr);
        let hit = hit.unwrap_or_else(|| panic!("{lambda}: {d:?}"));
        assert!(
            hit.args
                .iter()
                .any(|(k, v)| *k == "what" && format!("{v:?}").contains(what)),
            "{lambda}: {hit:?}"
        );
    }
}
