use super::*;

fn full() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("say").unwrap();
    r.stop::<(Option<Identity>, String), ()>("say_as").unwrap();
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>("choose")
        .unwrap();
    r
}

#[test]
fn quote_and_choice_can_bind_to_commands_of_matching_shape() {
    let mut r = full();
    r.bind(Sugar::Plain, "say").unwrap();
    r.bind(Sugar::Quote, "say_as").unwrap();
    r.bind(Sugar::Choice, "choose").unwrap();
    assert!(r.quote_binding().is_some() && r.choice_binding().is_some());
}

#[test]
fn commands_of_wrong_shape_cannot_bind_and_double_binding_is_rejected() {
    let mut r = full();
    // Quote lines are (Option<Identity>, str); choices are (list<str>, list<Option<str>>) -> int
    assert!(matches!(
        r.bind(Sugar::Quote, "say"),
        Err(RegistryError::SugarMismatch(_))
    ));
    assert!(matches!(
        r.bind(Sugar::Choice, "say_as"),
        Err(RegistryError::SugarMismatch(_))
    ));
    assert!(matches!(
        r.bind(Sugar::Plain, "say_as"),
        Err(RegistryError::SugarMismatch(_))
    ));
    assert!(matches!(
        r.bind(Sugar::Choice, "nowhere"),
        Err(RegistryError::UnknownCommand(_))
    ));
    r.bind(Sugar::Choice, "choose").unwrap();
    assert!(matches!(
        r.bind(Sugar::Choice, "choose"),
        Err(RegistryError::AlreadyBound(Sugar::Choice))
    ));
}

#[test]
fn ignore_can_be_specified_once_per_sugar() {
    let mut r = full();
    r.ignore(Sugar::Quote).unwrap();
    r.ignore(Sugar::Choice).unwrap();
    assert!(matches!(
        r.ignore(Sugar::Quote),
        Err(RegistryError::AlreadyBound(Sugar::Quote))
    ));
    assert!(r.quote_binding().is_some());
}

#[test]
fn script_types_can_be_derived_from_rust_types() {
    assert_eq!(<Identity as YomiType>::ty(), Ty::Identity);
    assert_eq!(<Vec<String> as YomiType>::ty(), Ty::List(Box::new(Ty::Str)));
    assert_eq!(
        <Option<Vec<i64>> as YomiType>::ty(),
        Ty::Option(Box::new(Ty::List(Box::new(Ty::Int))))
    );
    assert_eq!(
        <Option<Identity> as YomiType>::ty().name(),
        "Option<Identity>"
    );
}
