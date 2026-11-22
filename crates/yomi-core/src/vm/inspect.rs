//! VM: inspecting and changing state, for the host.

use super::*;

impl<'a> Vm<'a> {
    /// Whether it is stopped at a stop point. False when a load restored it with `LabelStart` (start with `run`).
    pub fn is_stopped(&self) -> bool {
        self.current.is_some()
    }

    /// The `Yield` while stopped.
    pub fn current(&self) -> Option<&Yield> {
        self.current.as_ref()
    }

    /// The list of variables (module variables, then local variables). Local variables are only those of the label being run.
    pub fn variables(&self) -> Vec<Variable> {
        let mut out = self.module_and_host_variables();
        let code = &self.program.labels[self.label];
        // Do not show the iteration state of `for` (hidden local variables whose names start with `$`), or variables of function
        // types that are not used at the stop point
        out.extend(
            code.locals
                .iter()
                .zip(&self.locals)
                .filter(|(l, _)| !l.name.starts_with('$') && !l.ty.contains_fn())
                .map(|(l, v)| Variable {
                    module: None,
                    name: l.name.clone(),
                    value: v.clone(),
                    local: true,
                    persistent: false,
                }),
        );
        out
    }

    /// Module variables and host-declared variables (`variables` without local variables).
    pub(crate) fn module_and_host_variables(&self) -> Vec<Variable> {
        let mut out: Vec<Variable> = self
            .program
            .globals
            .iter()
            .zip(&self.globals)
            .map(|(g, v)| Variable {
                module: Some(self.program.modules[g.module as usize].clone()),
                name: g.name.clone(),
                value: v.clone(),
                local: false,
                persistent: g.persistent,
            })
            .collect();
        // Host-declared variables. `module` is the namespace (the package name), `name` is the variable name
        out.extend(
            self.registry
                .vars()
                .iter()
                .zip(&self.host_vars)
                .map(|(d, v)| {
                    let (ns, name) = d.name.split_once('.').unwrap_or(("", d.name.as_str()));
                    Variable {
                        module: Some(ns.to_string()),
                        name: name.to_string(),
                        value: v.clone(),
                        local: false,
                        persistent: d.kind == Kind::Persistent,
                    }
                }),
        );
        out
    }

    /// The name of the current label (without the module name).
    pub fn label_name(&self) -> &str {
        &self.program.labels[self.label].name
    }

    /// The name of the module containing the current label.
    pub fn module_name(&self) -> &str {
        &self.program.modules[self.program.labels[self.label].module as usize]
    }

    /// Passed stop points (`module.label#id`). The host saves them as persistent data (read by `seen`).
    /// They grow when a stop point is reached (right before returning a `Yield`) and when an option is chosen.
    pub fn seen_points(&self) -> Vec<String> {
        self.seen.iter().cloned().collect()
    }

    /// Replaces the passed stop points (used to restore previously saved [`Vm::seen_points`]).
    /// If they are needed to rebuild the arguments of a stop point on load, pass them with [`VmConfig::seen_points`].
    pub fn set_seen_points<I, S>(&mut self, points: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.seen = points.into_iter().map(Into::into).collect();
    }

    /// The history of stop points (oldest first; up to the newest `VmConfig::history_limit` entries). Recorded every time a
    /// stop point is reached. Included in saves and undone by rollback.
    pub fn history(&self) -> &[Arc<StopRecord>] {
        let start = self.history.len().saturating_sub(self.cfg.history_limit);
        &self.history[start..]
    }

    /// The label (index into `Program::labels`) and the sequential number of the stop point in it, while stopped.
    pub(crate) fn stop_position(&self) -> Option<(usize, u32)> {
        match self.state {
            State::Stopped { ordinal } => Some((self.label, ordinal)),
            _ => None,
        }
    }

    /// For a choice stop point, the numbers in the order written of the options in the list given to the host.
    pub(crate) fn choice_origins(&self) -> &[u32] {
        &self.choice.orig
    }

    /// The labels entered, as indices into `Program::labels`.
    pub(crate) fn visited_indices(&self) -> &BTreeSet<u32> {
        &self.visited
    }

    /// Labels entered (`module.label`). Read by `visited`.
    pub fn visited_labels(&self) -> Vec<String> {
        self.visited
            .iter()
            .map(|&l| label_path(&self.program, l as usize))
            .collect()
    }

    /// `once` options already chosen (`module.label#id`).
    pub fn chosen_once(&self) -> Vec<String> {
        self.once_done
            .iter()
            .map(|(l, id)| format!("{}#{id}", label_path(&self.program, *l as usize)))
            .collect()
    }

    /// The depth of the call stack (the number of frames pushed by `call` that have not returned yet).
    pub fn call_depth(&self) -> usize {
        self.frames.len()
    }

    /// Reads the state slice of package `P` (the host reads it at a stop point and reflects it on the screen and so on; read
    /// only). `None` if `P` is not registered.
    pub fn package_state<P: Package>(&self) -> Option<&P::State> {
        let i = self
            .registry
            .package_entries()
            .iter()
            .position(|e| e.name == P::NAME)?;
        self.states.get(i)?.downcast_ref::<P::State>()
    }

    /// Returns the cues added after the last `run` / `resume`, in the order they were added (the order written in the script).
    ///
    /// Cues are in each package's state (`cues`), and their order across packages is known from this table. Use each entry's
    /// `package` (`Package::NAME`) and `index` (the position in that package's `cues`) to take them from `package_state`.
    /// Emptied by the next `run` / `resume` and by loading or restoring.
    pub fn cue_order(&self) -> impl Iterator<Item = CueRef<'_>> {
        let entries = self.registry.package_entries();
        self.cue_order.iter().map(move |&(p, index)| CueRef {
            package: entries[p as usize].name.as_str(),
            index,
        })
    }

    /// Reads the persistent slice of package `P` (read only). `None` if `P` is not registered.
    pub fn package_persistent<P: Package>(&self) -> Option<&P::Persistent> {
        let i = self
            .registry
            .package_entries()
            .iter()
            .position(|e| e.name == P::NAME)?;
        self.persist.get(i)?.downcast_ref::<P::Persistent>()
    }

    /// Reads the value of a host-declared variable ([`Registry::var`]). `None` if not registered.
    pub fn get_var(&self, name: &str) -> Option<Value> {
        let i = self.registry.var_index(name)?;
        self.host_vars.get(i).cloned()
    }

    /// Writes a host-declared variable. `read_only` variables can be written too. `UnknownVariable` if the name does not
    /// exist, `BadInput` if the type does not match (in both cases, the VM is unchanged).
    pub fn set_var(&mut self, name: &str, value: Value) -> Result<(), VmError> {
        let Some(i) = self.registry.var_index(name) else {
            return Err(self.misuse(VmErrorKind::UnknownVariable(name.to_string())));
        };
        let Some(value) = value.normalized() else {
            return Err(self.misuse(VmErrorKind::NonFiniteFloat));
        };
        let expected = self.registry.vars()[i].ty.clone();
        if !value.matches_in(&expected, self.program.types()) {
            return Err(self.misuse(VmErrorKind::BadInput {
                found: value.ty(),
                expected,
            }));
        }
        self.host_vars[i] = value;
        self.total_valid = false;
        Ok(())
    }
}

/// One entry of the table of cues in the order written ([`Vm::cue_order`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CueRef<'a> {
    /// The name of the package that added the cue (`Package::NAME`).
    pub package: &'a str,
    /// The position within the `cues` of that package's state.
    pub index: usize,
}
