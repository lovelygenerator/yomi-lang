use super::*;

fn server() -> Server {
    Server::new(Localizer::from_env(&crate::LOCALES, Some("en")))
}

#[test]
fn translation_keys_for_tool_descriptions_and_args_are_complete() {
    lingokit::rmcp::assert_keys(&Server::tool_router(), &crate::LOCALES);
}

#[tokio::test]
async fn check_returns_ok_for_valid_and_diagnostics_for_broken_script() {
    let s = server();
    let ok = s
        .check(Parameters(CheckParams {
            source: Some("label start {\n    こんにちは\n}\n".into()),
            modules: None,
            path: None,
            root: None,
            decls: None,
        }))
        .await
        .unwrap()
        .0;
    assert!(ok.ok, "{:?}", ok.diagnostics);
    let bad = s
        .check(Parameters(CheckParams {
            source: Some("label start {\n    jump nowhere\n}\n".into()),
            modules: None,
            path: None,
            root: None,
            decls: None,
        }))
        .await
        .unwrap()
        .0;
    assert!(!bad.ok);
    assert_eq!(bad.errors as usize, bad.diagnostics.len());
    assert_eq!(bad.diagnostics[0].severity, "error");
    assert!(!bad.diagnostics[0].code.is_empty());
    assert!(!bad.diagnostics[0].message.is_empty());
}

#[tokio::test]
async fn check_accepts_exactly_one_of_path_and_source() {
    let s = server();
    for (path, source) in [
        (None, None),
        (Some("a.yomi".to_string()), Some(String::new())),
    ] {
        let e = s
            .check(Parameters(CheckParams {
                source,
                modules: None,
                path,
                root: None,
                decls: None,
            }))
            .await
            .err()
            .expect("誤りになる");
        assert!(e.message.contains("path"), "{}", e.message);
    }
}

#[tokio::test]
async fn reserved_words_match_keyword_table() {
    let r = server().reserved_words().await.unwrap().0;
    assert_eq!(r.keywords.len(), KEYWORDS.len());
    assert!(
        r.keywords
            .iter()
            .any(|k| k.word == "label" && k.kind == "declaration")
    );
    assert_eq!(r.contextual.len(), CONTEXTUAL_KEYWORDS.len());
    assert!(r.contextual.iter().any(|k| k.word == "once"));
}

#[tokio::test]
async fn diagnostic_codes_adds_command_codes_with_packages() {
    let s = server();
    let base = s
        .diagnostic_codes(Parameters(CodesParams { packages: None }))
        .await
        .unwrap()
        .0;
    assert_eq!(base.codes.len(), all_codes().len());
    assert!(
        base.codes
            .iter()
            .all(|c| c.kind == "language" && !c.message.is_empty())
    );
    let all = s
        .diagnostic_codes(Parameters(CodesParams {
            packages: Some(true),
        }))
        .await
        .unwrap()
        .0;
    assert!(all.codes.len() > base.codes.len());
    assert!(all.codes.iter().any(|c| c.kind == "package"));
    assert!(all.codes.iter().any(|c| c.kind == "core"));
}

#[tokio::test]
async fn spec_returns_list_and_text_and_rejects_unknown_document() {
    let s = server();
    let list = s
        .spec(Parameters(SpecParams { part: None }))
        .await
        .unwrap()
        .0;
    assert_eq!(list.parts.len(), spec::PARTS.len());
    assert!(list.text.is_none());
    let one = s
        .spec(Parameters(SpecParams {
            part: Some("language/01-lexical".into()),
        }))
        .await
        .unwrap()
        .0;
    assert!(one.text.unwrap().contains("予約語"));
    let e = s
        .spec(Parameters(SpecParams {
            part: Some("nothing".into()),
        }))
        .await
        .err()
        .expect("誤りになる");
    assert!(e.message.contains("language/01-lexical"), "{}", e.message);
}
