//! Helpers shared by the VM unit tests (compiled only for tests).
//!
//! Each test file has a `registry()` that fits the commands it uses, and passes it to the functions here.

use crate::compile::{compile, compile_with_warnings};
use crate::program::Program;
use crate::registry::Sugar;
use crate::texts::TextTable;
use crate::vm::*;

/// Makes a registry with sugar for bodies.
///
/// `ui.say` (plain lines) is always registered. The sugars listed in `sugars` (`Quote` → `ui.say_as`, `Choice` →
/// `ui.choose`) are registered in the order listed, then `extra` adds the commands for each test, and finally the sugars
/// are bound.
pub(super) fn ui_registry(sugars: &[Sugar], extra: impl FnOnce(&mut Registry)) -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>("ui.say").unwrap();
    for sugar in sugars {
        match sugar {
            Sugar::Quote => r
                .stop::<(Option<crate::Identity>, String), ()>("ui.say_as")
                .unwrap(),
            Sugar::Choice => r
                .stop::<(Vec<String>, Vec<Option<String>>), i64>("ui.choose")
                .unwrap(),
            other => panic!("登録しない糖衣: {other:?}"),
        };
    }
    extra(&mut r);
    r.bind(Sugar::Plain, "ui.say").unwrap();
    if sugars.contains(&Sugar::Quote) {
        r.bind(Sugar::Quote, "ui.say_as").unwrap();
    }
    if sugars.contains(&Sugar::Choice) {
        r.bind(Sugar::Choice, "ui.choose").unwrap();
    }
    r
}

/// Compiles. If it cannot, fails showing the diagnostics and the script.
pub(super) fn compile_ok(src: &str, reg: &Registry) -> Program {
    match compile(src, "main", reg) {
        Ok(p) => p,
        Err(d) => panic!("コンパイルできない: {d:?}\n{src}"),
    }
}

/// The codes of compile errors (empty on success; warnings are not included).
pub(super) fn error_codes(src: &str, reg: &Registry) -> Vec<&'static str> {
    match compile(src, "main", reg) {
        Ok(_) => Vec::new(),
        Err(d) => d.iter().map(|d| d.code.as_str()).collect(),
    }
}

/// The codes of compile diagnostics (warnings on success; errors and warnings on failure).
pub(super) fn diag_codes(src: &str, reg: &Registry) -> Vec<&'static str> {
    match compile_with_warnings(src, "main", reg) {
        Ok((_, w)) => w.iter().map(|d| d.code.as_str()).collect(),
        Err(d) => d.iter().map(|d| d.code.as_str()).collect(),
    }
}

/// The body of a stop point (the string of the last argument). The end is `<end>`.
pub(super) fn text_of(y: &Yield) -> String {
    match y {
        Yield::Stop { args, .. } => args.last().map(Value::to_text).unwrap_or_default(),
        Yield::End => "<end>".to_string(),
    }
}

/// The arguments of a stop point (empty at the end).
pub(super) fn args_of(y: &Yield) -> Vec<Value> {
    match y {
        Yield::Stop { args, .. } => args.clone(),
        Yield::End => Vec::new(),
    }
}

/// The current value of a variable. Fails if it does not exist.
pub(super) fn var(vm: &Vm<'_>, name: &str) -> Value {
    vm.variables()
        .into_iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("変数 {name} がない"))
        .value
}

/// Resumes. For a choice (the first argument is a list), chooses option 0; otherwise returns `()`.
pub(super) fn resume(vm: &mut Vm<'_>) {
    let is_choice = matches!(
        vm.current(),
        Some(Yield::Stop { args, .. }) if matches!(args.first(), Some(Value::List(_)))
    );
    let v = if is_choice {
        Value::Int(0)
    } else {
        Value::Unit
    };
    vm.resume(Input::Return(v)).unwrap();
}

/// Runs to the end (up to `limit` stops at most) and collects the bodies. The end is `<end>`, a runtime error is
/// `<code>`. When starting from a stop, resumes first.
pub(super) fn run_all(vm: &mut Vm<'_>, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    if vm.is_stopped() {
        resume(vm);
    }
    for _ in 0..limit {
        match vm.run() {
            Ok(Yield::End) => {
                out.push("<end>".to_string());
                break;
            }
            Ok(y) => {
                out.push(text_of(&y));
                resume(vm);
            }
            Err(e) => {
                out.push(format!("<{}>", e.kind.code()));
                break;
            }
        }
    }
    out
}

/// The bodies when compiled and run to the end with settings `cfg` ([`run_all`]).
pub(super) fn texts_with(src: &str, reg: &Registry, cfg: VmConfig, limit: usize) -> Vec<String> {
    let prog = compile_ok(src, reg);
    let mut vm = Vm::new(&prog, reg, cfg).unwrap();
    run_all(&mut vm, limit)
}

/// An English text table.
pub(super) fn table(pairs: &[(&str, &str)]) -> TextTable {
    let mut t = TextTable::new("en");
    for (k, v) in pairs {
        t.insert(*k, *v).unwrap();
    }
    t
}
