//! Identity

use super::*;

pub(super) const ID_SRC: &str = "identity 案内人 = \"あんないにん\"\nidentity 門番 = \"もんばん\"\nsave var talker: Identity = 案内人\nlabel start {\n  let who: Identity = 門番\n  ${who.id} ${who.name} ${案内人.name} ${talker.id}\n  ${who == 門番} ${who != talker}\n  let all: list<Identity> = [案内人, 門番]\n  for c in all {\n    ${c.name}\n  }\n  talker = who\n  ${talker.name}\n}\n";

#[test]
fn identity_type_annotation_id_name_and_comparison() {
    let t = texts(ID_SRC);
    assert_eq!(
        &t[..4],
        [
            "門番 もんばん あんないにん 案内人",
            "true true",
            "あんないにん",
            "もんばん"
        ]
        .map(String::from)
        .as_slice()
    );
    assert_eq!(t[4], "もんばん");
}

#[test]
fn identity_cannot_be_constructed_and_only_declared_names_are_constants() {
    assert_eq!(
        compile_codes("label start {\n  let x: Identity = \"案内人\"\n}\n"),
        ["type_mismatch"]
    );
    assert_eq!(
        compile_codes("identity 案内人 = \"a\"\nlabel start {\n  let x = 案内人.nope\n}\n"),
        ["unknown_field"]
    );
    assert_eq!(
        compile_codes("label start {\n  let x = 案内人\n}\n"),
        ["unknown_name"]
    );
    // The variable name is found before the identity
    assert_eq!(
        compile_codes("identity 案内人 = \"a\"\nlabel start {\n  let 案内人 = 1\n}\n"),
        Vec::<&str>::new()
    );
}

#[test]
fn identity_constants_across_modules() {
    let reg = registry();
    let a = "import b\nlabel start {\n  ${案内人.name} ${b.門番.id}\n}\n";
    let b = "identity 案内人 = \"あんないにん\"\nidentity 門番 = \"もんばん\"\n";
    let (prog, _) =
        crate::compile::compile_modules_with_warnings(&[("a", a), ("b", b)], &reg).unwrap();
    let mut vm = Vm::new(&prog, &reg, VmConfig::default().start("a.start")).unwrap();
    assert_eq!(run_all(&mut vm), ["あんないにん 門番", "<end>"]);
}

#[test]
fn identity_can_be_map_key() {
    let body = "  let m: map<Identity, int> = map { 案内人: 1 }\n  m[門番] = 2\n  m[案内人] += 5\n  ${m.len()} ${m[案内人]} ${m.has(門番)}\n  for k in m.keys() {\n    ${k.id}\n  }";
    let src = format!(
        "identity 案内人 = \"あんないにん\"\nidentity 門番 = \"もんばん\"\nlabel start {{\n{body}\n}}\n"
    );
    assert_eq!(texts(&src), ["2 6 true", "案内人", "門番", "<end>"]);
}
