//! VM: common parts for executing instructions (the execution loop is `regexec`).

use super::*;

/// After executing one instruction, whether to go on or stop.
pub(super) enum Step {
    /// Go on to the next instruction.
    Next,
    /// Stop at a stop point or the end.
    Yield(Inner),
}

impl<'a> Vm<'a> {
    /// Calls the function of an effect or query command. `None` if the return value is `()`.
    pub(super) fn call_native(
        &mut self,
        native: usize,
        args: &[Value],
    ) -> Result<Option<Value>, VmErrorKind> {
        let reg = self.registry;
        let decl = &reg.natives()[self.native_ids[native]];
        let index = decl.package;
        let (before, rest) = self.states.split_at_mut(index);
        let Some((own, after)) = rest.split_first_mut() else {
            return Err(VmErrorKind::CommandFailed {
                code: "core.package_slice_missing".to_string(),
                args: Vec::new(),
            });
        };
        let cue_fn = reg.package_entries()[index].cue_len;
        let cues_before = cue_fn(own);
        let env = Env {
            index,
            packages: reg.package_entries(),
            before,
            own,
            after,
            persistent: &mut self.persist[index],
            rng: &mut self.rng,
            interrupt: &self.interrupt,
        };
        let out = (decl.func)(env, args).map_err(|e| e.into_vm_kind())?;
        if self.interrupt.is_interrupted() {
            return Err(VmErrorKind::Interrupted);
        }
        // Add the new cues to the table in the order written (`own` was lent to `env`, so borrow it again here)
        let cues_after = cue_fn(&self.states[index]);
        self.cue_order
            .extend((cues_before..cues_after).map(|i| (index as u32, i)));
        let mut budget = self.cfg.max_value_bytes;
        match normalize_result(out, &mut budget)? {
            Value::Unit => Ok(None),
            v => Ok(Some(v)),
        }
    }
}
