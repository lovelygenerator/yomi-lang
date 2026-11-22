//! Tests of the package mechanism (registration, dependencies, saving and restoring state slices, `migrate`,
//! host-declared variables).
//!
//! The packages used are the test-only ones in `tests/common/mod.rs`.

#[path = "../common/mod.rs"]
mod common;

use common::{
    Counter, CounterPersistent, CounterState, Cued, CuedState, Mirror, MirrorState, Other,
    VerPersistentV1, VerPersistentV2, VerStateV1, VerStateV2, VerV1, VerV2, cued_registry,
    full_registry, host_registry,
};
use serde::{Deserialize, Serialize};
use yomi_core::{
    CommandKind, Ctx, DiagArg, Input, Kind, LoadErrorKind, Package, PackageReg, Program, Registry,
    RegistryError, Snapshot, Value, Vm, VmConfig, VmErrorKind, Yield, compile,
};

fn compile_ok(src: &str, reg: &Registry) -> Program {
    match compile(src, "main", reg) {
        Ok(p) => p,
        Err(d) => panic!("コンパイルできない: {d:?}"),
    }
}

/// Advances to the next stop point (there must be one).
fn to_stop(vm: &mut Vm<'_>) {
    match vm.run().expect("動く") {
        Yield::Stop { .. } => {}
        other => panic!("停止点のはず: {other:?}"),
    }
}

fn advance(vm: &mut Vm<'_>) {
    vm.resume(Input::Return(Value::Unit)).expect("再開できる");
}

mod cues;
mod fn_queries;
mod host_access;
mod package_set;
mod registration;
mod save_persistent;
mod snapshot;
mod stop_validated;

use registration::*;
