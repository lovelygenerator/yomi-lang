use super::util::{self, *};
use crate::compile::compile;
use crate::program::Program;
use crate::vm::*;

fn registry() -> Registry {
    ui_registry(&[], |_| {})
}

fn texts(src: &str) -> Vec<String> {
    let reg = registry();
    let prog = compile(src, "main", &reg).expect("コンパイルできる");
    let mut vm = Vm::new(&prog, &reg, VmConfig::default()).unwrap();
    let mut out = Vec::new();
    loop {
        match vm.run().unwrap() {
            Yield::Stop { args, .. } => {
                out.push(args[0].to_text());
                vm.resume(Input::Return(Value::Unit)).unwrap();
            }
            Yield::End => return out,
        }
    }
}

mod calls;
mod choices;
mod execution;
