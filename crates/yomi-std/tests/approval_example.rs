//! Runs the approval flow (`examples/approval/`) through the host example (`examples/approval.rs`).
//!
//! If the script is missing (a public repository exported with only the crate), it finishes without checking anything.

#[allow(dead_code)]
#[path = "../examples/approval.rs"]
mod approval;

fn script() -> Option<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/approval/approval.yomi");
    std::fs::read_to_string(path).ok()
}

fn shown(events: &[(&str, &str)]) -> Vec<String> {
    approval::run(&script().unwrap(), events).unwrap().0
}

#[test]
fn small_requests_are_approved_without_waiting() {
    if script().is_none() {
        return;
    }
    let out = shown(&[("submitted", r#"{"amount":8000}"#)]);
    assert_eq!(
        out,
        ["申請を待っています。", "承認されました。購買部へ回します。"]
    );
}

#[test]
fn large_requests_wait_for_the_approver_chosen_by_amount() {
    if script().is_none() {
        return;
    }
    let out = shown(&[("submitted", r#"{"amount":600000}"#), ("approved", "{}")]);
    assert_eq!(out[1], "部長の承認を待っています（600000 円）。");
    assert_eq!(out[2], "承認されました。購買部へ回します。");
    let out = shown(&[("submitted", r#"{"amount":120000}"#), ("approved", "{}")]);
    assert_eq!(out[1], "課長の承認を待っています（120000 円）。");
}

#[test]
fn a_rejection_shows_its_reason_when_there_is_one() {
    if script().is_none() {
        return;
    }
    let with = shown(&[
        ("submitted", r#"{"amount":50000}"#),
        ("rejected", r#"{"reason":"予算超過"}"#),
    ]);
    assert_eq!(with.last().unwrap(), "却下されました。理由：予算超過");
    let without = shown(&[("submitted", r#"{"amount":50000}"#), ("rejected", "{}")]);
    assert_eq!(without.last().unwrap(), "却下されました。");
}

#[test]
fn a_request_without_an_amount_is_refused() {
    if script().is_none() {
        return;
    }
    let out = shown(&[("submitted", r#"{"title":"x"}"#)]);
    assert_eq!(out.last().unwrap(), "申請の形式が正しくありません。");
}

#[test]
fn the_host_reads_the_wait_conditions_from_the_saves() {
    let Some(source) = script() else {
        return;
    };
    let (_, waits) = approval::run(
        &source,
        &[("submitted", r#"{"amount":50000}"#), ("approved", "{}")],
    )
    .unwrap();
    assert_eq!(waits.len(), 2);
    assert!(waits[0].contains("submitted"));
    assert!(waits[1].contains("approved") && waits[1].contains("86400"));
}
