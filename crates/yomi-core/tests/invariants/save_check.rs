//! Checking saves (used for both valid and mutated scripts)

use super::*;

/// At each stop point, save → restore, and checks that the continuation is the same as the original VM.
pub(super) fn check_save_restore(prog: &Program, reg: &Registry, what: &str) {
    // Find the number of stop points of the original
    let total = trace_of(prog, reg, 2_000)
        .iter()
        .filter(|s| matches!(s, Ok(Yield::Stop { .. })))
        .count();
    let stride = (total / 30).max(1);

    let Ok(mut vm) = Vm::new(prog, reg, config()) else {
        return;
    };
    let mut index = 0usize;
    loop {
        let s = step(&mut vm);
        let Ok(y @ Yield::Stop { .. }) = s else {
            return;
        };
        if index.is_multiple_of(stride) {
            let bytes = vm
                .save()
                .unwrap_or_else(|e| panic!("{what}: 保存できない {e}"));
            // Saves from the same state are the same bytes
            assert_eq!(bytes, vm.save().unwrap(), "{what}: 同じ状態のセーブが違う");
            // `persistent var` and `seen` do not go into saves. Pass the persistent data the host had, as is
            let persistent = vm.save_persistent();
            let cfg = config().persistent_data(persistent.clone());
            let (mut vm2, report) = Vm::load(prog, reg, cfg, &bytes)
                .unwrap_or_else(|e| panic!("{what}: 復元できない（停止点 {index}）: {e}"));
            let id = match &y {
                Yield::Stop { id, .. } => id.clone(),
                _ => unreachable!(),
            };
            if vm.call_depth() > 0 {
                // Call lines have no IDs (found by hash)
                assert!(
                    matches!(report.resolved_by, ResolvedBy::Hash | ResolvedBy::Ordinal),
                    "{what}: 停止点 {index}: {:?}",
                    report.resolved_by
                );
            } else if id.is_some() {
                assert_eq!(report.resolved_by, ResolvedBy::Id, "{what}: 停止点 {index}");
            } else {
                assert!(
                    matches!(report.resolved_by, ResolvedBy::Hash | ResolvedBy::Ordinal),
                    "{what}: 停止点 {index}: {:?}",
                    report.resolved_by
                );
            }
            assert!(
                report.added_vars.is_empty()
                    && report.removed_vars.is_empty()
                    && report.type_changed_vars.is_empty()
                    && report.type_changed_locals.is_empty(),
                "{what}: 停止点 {index}: {report:?}"
            );
            let mut expected = yomi_core::PersistentReport::default();
            expected.from_schema_version = 1;
            expected.to_schema_version = 1;
            assert_eq!(report.persistent, Some(expected), "{what}: 停止点 {index}");
            assert_eq!(vm2.current(), vm.current(), "{what}: 停止点 {index}");
            assert_eq!(vm2.history(), vm.history(), "{what}: 停止点 {index}: 履歴");
            assert_eq!(
                vm2.seen_points(),
                vm.seen_points(),
                "{what}: 停止点 {index}"
            );
            // Saving persistent data again gives the same bytes (save → read equivalence)
            assert_eq!(
                vm2.save_persistent(),
                persistent,
                "{what}: 停止点 {index}: 永続データ"
            );
            // Reading the save first and the persistent data later gives the same state
            if let Ok((mut late, _)) = Vm::load(prog, reg, config(), &bytes)
                && late.is_stopped()
            {
                late.load_persistent(&persistent)
                    .unwrap_or_else(|e| panic!("{what}: 停止点 {index}: {e}"));
                assert_eq!(
                    late.current(),
                    vm2.current(),
                    "{what}: 停止点 {index}: 順序"
                );
                assert_eq!(late.variables(), vm2.variables(), "{what}: 停止点 {index}");
                assert_eq!(late.history(), vm2.history(), "{what}: 停止点 {index}");
                assert_eq!(late.save().unwrap(), bytes, "{what}: 停止点 {index}");
                assert_eq!(late.save_persistent(), persistent);
            }
            assert_eq!(vm2.variables(), vm.variables(), "{what}: 停止点 {index}");
            assert_eq!(vm2.label_name(), vm.label_name(), "{what}: 停止点 {index}");
            assert_eq!(
                vm2.module_name(),
                vm.module_name(),
                "{what}: 停止点 {index}"
            );
            assert_eq!(
                vm2.call_depth(),
                vm.call_depth(),
                "{what}: 停止点 {index}: コールスタックの深さが違う"
            );
            // Saving a restored VM again gives the same bytes
            assert_eq!(
                vm2.save().unwrap(),
                bytes,
                "{what}: 停止点 {index}: 保存し直したバイト列が違う"
            );
            // The continuation is the same (the VM before saving cannot be copied, so the same script is run from the start and
            // compared)
            let mut fork = Vm::new(prog, reg, config()).unwrap();
            for _ in 0..=index {
                let s = step(&mut fork);
                let v = answer(reg, s.as_ref().unwrap());
                fork.resume(Input::Return(v)).unwrap();
            }
            let a = trace(&mut fork, reg, 2_000);
            let v = answer(reg, &y);
            vm2.resume(Input::Return(v)).unwrap();
            let b = trace(&mut vm2, reg, 2_000);
            assert_eq!(a, b, "{what}: 停止点 {index} からの続きが違う");
            assert_eq!(
                fork.variables(),
                vm2.variables(),
                "{what}: 停止点 {index}: 最終の変数が違う"
            );
        }
        let v = answer(reg, &y);
        vm.resume(Input::Return(v)).unwrap();
        index += 1;
        if index > 2_000 {
            return;
        }
    }
}
