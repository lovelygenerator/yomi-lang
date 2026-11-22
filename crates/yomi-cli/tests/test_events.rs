//! Checks `yomi test --event` (the data of events tried at `event.wait`).

use std::process::Command;

use serde_json::Value;

fn approval() -> String {
    format!(
        "{}/../../examples/approval/approval.yomi",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn report(events: &[&str]) -> Value {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_yomi"));
    cmd.env("YOMI_CLI_LANG", "en")
        .args(["test", &approval(), "--json"]);
    for e in events {
        cmd.args(["--event", e]);
    }
    let out = cmd.output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| {
        panic!("{e}: {}", String::from_utf8_lossy(&out.stdout));
    })
}

#[test]
fn without_event_the_waits_get_empty_data_and_the_flow_is_not_reached() {
    let r = report(&[]);
    assert_eq!(r["paths"].as_array().unwrap().len(), 1, "{r}");
    assert!(
        r["unreached_labels"]
            .as_array()
            .unwrap()
            .contains(&Value::from("approval.wait_approval")),
        "{r}"
    );
}

#[test]
fn event_data_reaches_the_paths_that_depend_on_it() {
    let r = report(&[
        r#"submitted={"amount":500}"#,
        r#"submitted={"amount":600000}"#,
    ]);
    let paths = r["paths"].as_array().unwrap();
    assert!(paths.len() >= 4, "{r}");
    let answers: Vec<String> = paths
        .iter()
        .flat_map(|p| p["answers"].as_array().unwrap().iter())
        .map(|a| a.as_str().unwrap().to_string())
        .collect();
    assert!(
        answers.contains(&r#"submitted {"amount":500}"#.to_string()),
        "{r}"
    );
    assert!(
        answers.contains(&r#"submitted {"amount":600000}"#.to_string()),
        "{r}"
    );
    assert!(r["unreached_labels"].as_array().unwrap().is_empty(), "{r}");
}

#[test]
fn event_must_have_a_name_and_an_equals_sign() {
    for bad in ["=x", "nodata"] {
        let out = Command::new(env!("CARGO_BIN_EXE_yomi"))
            .args(["test", &approval(), "--event", bad])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{bad}");
    }
}
