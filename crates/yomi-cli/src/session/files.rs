//! Reading and writing the session's save, persistent data and text table files, and reporting on them.

use super::*;

impl<'a, R: BufRead, O: Write, E: Write> Session<'a, R, O, E> {
    /// Rebuilds the VM from the bytes of a save and replaces the current stop point.
    ///
    /// Saves do not contain `persistent var`s or passed stop points, so those of the current VM are carried over (the current
    /// values are also used when rebuilding the arguments of the stop point). Pushing onto the history again is done by the
    /// caller (`drive`) for each stop point. The returned `Yield` is the restored stop point (when it went back to the start
    /// of the label, where `run` from there led).
    pub(super) fn restore(
        &mut self,
        vm: &mut Vm<'a>,
        bytes: &[u8],
        path: &str,
    ) -> Result<Step, Fatal> {
        let cfg = VmConfig::default()
            .start(self.start)
            .seed(self.seed)
            .load_policy(self.policy)
            .persistent_data(vm.save_persistent());
        let (mut loaded, report) = Vm::load(self.program, self.reg, cfg, bytes)?;
        self.say("run-loaded", Some(&one_arg("path", path)))?;
        self.report_load(&report)?;
        // The language is not in the save, so set the current text table again on the loaded VM
        if let Some(table) = &self.texts {
            loaded
                .set_texts(Some(table.clone()))
                .expect("設定済みの表は、同じ台本に設定できる");
            if loaded.is_stopped() {
                loaded.refresh_current()?;
            }
        }
        // When it went back to the start of the label, the VM is not stopped, so resume with `run`.
        let y = match loaded.current() {
            Some(y) => y.clone(),
            None => loaded.run()?,
        };
        *vm = loaded;
        Ok(Step::Moved(y))
    }

    /// `:reload`. Collects and compiles the scripts again and hands the current stop point over to the new program
    /// (`spec/host/04-save.md`, 4.10). On any failure, writes the reason to standard error and goes on with the current one.
    pub(super) fn reload(
        &mut self,
        vm: &mut Vm<'a>,
        history: &mut SnapshotLog,
    ) -> Result<Step, Fatal> {
        let Some(entry) = self.entry else {
            self.complain("run-reload-unavailable", None)?;
            return Ok(Step::Stay);
        };
        let name = entry.display().to_string();
        let files = match modules::collect(entry, self.root) {
            Ok((files, _)) => files,
            Err(errors) => {
                for e in &errors {
                    writeln!(
                        self.err,
                        "{}",
                        crate::texts::collect_error_line(self.loc, &name, e)
                    )?;
                }
                self.complain("run-reload-kept", None)?;
                return Ok(Step::Stay);
            }
        };
        let Some(program) = compile_files(self.loc, self.reg, &files, self.start, self.err) else {
            self.complain("run-reload-kept", None)?;
            return Ok(Step::Stay);
        };
        // The new program must live as long as the session (a few reloads by hand; the program is small).
        let program: &'a Program = Box::leak(Box::new(program));
        let (mut next, report) = match vm.reload(program) {
            Ok(done) => done,
            Err(e) => {
                let reason = match &e {
                    yomi_core::ReloadError::Load(e) => load_error_message(self.loc, e),
                    other => self.loc.fmt(
                        &format!("run-reload-{}", other.code().replace('_', "-")),
                        None,
                    ),
                };
                self.complain("run-reload-failed", Some(&one_arg("reason", &reason)))?;
                return Ok(Step::Stay);
            }
        };
        // The text table is set again on the new VM (the file is read again if it is a `--texts` file).
        let table = match self.texts_path {
            Some(path) => match self.load_table(&mut next, &path.display().to_string()) {
                Ok(table) => table,
                Err(Fatal::Texts(lines)) => {
                    for l in lines {
                        writeln!(self.err, "{l}")?;
                    }
                    self.complain("run-reload-kept", None)?;
                    return Ok(Step::Stay);
                }
                Err(other) => return Err(other),
            },
            None => {
                if let Some(table) = &self.texts
                    && let Err(issues) = next.set_texts(Some(table.clone()))
                {
                    for i in &issues {
                        writeln!(
                            self.err,
                            "{}",
                            crate::texts::issue_line(self.loc, ":texts", i)
                        )?;
                    }
                    self.complain("run-reload-kept", None)?;
                    return Ok(Step::Stay);
                }
                self.texts.clone()
            }
        };
        self.texts = table;
        if next.is_stopped() {
            next.refresh_current()?;
        }
        let by = match report.resolved_by {
            ResolvedBy::Id => "id",
            ResolvedBy::Hash => "hash",
            _ => "position",
        };
        self.complain("run-reloaded", Some(&one_arg("by", by)))?;
        let y = next
            .current()
            .cloned()
            .expect("読み込み直した VM は停止している");
        *vm = next;
        self.program = program;
        // Snapshots of the old program cannot be restored on the new one
        *history = SnapshotLog::new(ROLLBACK_LIMIT);
        Ok(Step::Moved(y))
    }

    /// At startup, reads the `--texts` table and sets it on the VM. If it cannot be read or set, does not run.
    pub(super) fn read_start_texts(&mut self, vm: &mut Vm<'a>) -> Result<(), Fatal> {
        let Some(path) = self.texts_path else {
            return Ok(());
        };
        let table = self.load_table(vm, &path.display().to_string())?;
        // It is before stopping, so there is no need to rebuild
        self.texts = table;
        Ok(())
    }

    /// Reads a table file (`off` goes back to the originals) and sets it on the VM. Returns the table set (`None` for
    /// `off`). On failure, the VM is unchanged and `Fatal::Texts` holds the line to write to standard error.
    pub(super) fn load_table(
        &self,
        vm: &mut Vm<'a>,
        path: &str,
    ) -> Result<Option<TextTable>, Fatal> {
        if path == "off" {
            vm.set_texts(None).expect("原文に戻すのは失敗しない");
            return Ok(None);
        }
        let table = crate::texts::read_table(self.loc, Path::new(path))
            .map_err(|l| Fatal::Texts(vec![l]))?;
        vm.set_texts(Some(table.clone())).map_err(|issues| {
            Fatal::Texts(
                issues
                    .iter()
                    .map(|i| crate::texts::issue_line(self.loc, path, i))
                    .collect(),
            )
        })?;
        Ok(Some(table))
    }

    /// `:texts <file or off>`. Switches the text table, rebuilds the current stop point and shows it.
    /// If the table cannot be read or set, writes to standard error and keeps waiting.
    pub(super) fn switch_texts(&mut self, vm: &mut Vm<'a>, arg: &str) -> Result<Step, Fatal> {
        match self.load_table(vm, arg) {
            Ok(table) => {
                let key = if table.is_some() {
                    "run-texts-on"
                } else {
                    "run-texts-off"
                };
                self.texts = table;
                self.say(key, Some(&one_arg("path", arg)))?;
                Ok(Step::Redraw(vm.refresh_current()?))
            }
            Err(Fatal::Texts(lines)) => {
                for l in lines {
                    writeln!(self.err, "{l}")?;
                }
                Ok(Step::Stay)
            }
            Err(other) => Err(other),
        }
    }

    /// Prints the load report to standard output (only things worth reporting).
    pub(super) fn report_load(&mut self, report: &LoadReport) -> Result<(), Fatal> {
        match report.resolved_by {
            ResolvedBy::Ordinal => self.say("run-load-by-ordinal", None)?,
            ResolvedBy::LabelStart => self.say("run-load-by-label-start", None)?,
            _ => {}
        }
        for (key, vars) in [
            ("run-load-added-vars", &report.added_vars),
            ("run-load-removed-vars", &report.removed_vars),
            ("run-load-type-changed-vars", &report.type_changed_vars),
            ("run-load-type-changed-locals", &report.type_changed_locals),
        ] {
            if !vars.is_empty() {
                self.say(key, Some(&one_arg("vars", &vars.join(", "))))?;
            }
        }
        for (key, packages) in [
            ("run-load-ignored-slices", &report.ignored_slices),
            ("run-load-defaulted-slices", &report.defaulted_slices),
            ("run-load-migrated-slices", &report.migrated_slices),
            ("run-load-changed-rules", &report.changed_rules),
        ] {
            if !packages.is_empty() {
                self.say(key, Some(&one_arg("packages", &packages.join(", "))))?;
            }
        }
        Ok(())
    }

    /// At startup, reads the persistent data file and puts it into the VM (new if the file does not exist).
    pub(super) fn read_persistent(&mut self, vm: &mut Vm<'a>) -> Result<(), Fatal> {
        let Some(path) = self.persistent else {
            return Ok(());
        };
        let shown = path.display().to_string();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                return Err(Fatal::Persistent(
                    "run-persistent-read-failed",
                    shown,
                    e.to_string(),
                ));
            }
        };
        match vm.load_persistent(&bytes) {
            Ok(report) => self.report_persistent(&report),
            Err(e) => Err(Fatal::Persistent(
                "run-persistent-read-failed",
                shown,
                load_error_message(self.loc, &e),
            )),
        }
    }

    /// Prints the report of reading persistent data to standard error (only things worth reporting).
    pub(super) fn report_persistent(&mut self, report: &PersistentReport) -> Result<(), Fatal> {
        for (key, vars) in [
            ("run-load-added-vars", &report.added_vars),
            ("run-load-removed-vars", &report.removed_vars),
            ("run-load-type-changed-vars", &report.type_changed_vars),
        ] {
            if !vars.is_empty() {
                self.complain(key, Some(&one_arg("vars", &vars.join(", "))))?;
            }
        }
        for (key, packages) in [
            ("run-load-ignored-slices", &report.ignored_slices),
            ("run-load-defaulted-slices", &report.defaulted_slices),
            ("run-load-migrated-slices", &report.migrated_slices),
        ] {
            if !packages.is_empty() {
                self.complain(key, Some(&one_arg("packages", &packages.join(", "))))?;
            }
        }
        Ok(())
    }

    /// Writes persistent data to the file (writes to a temporary file, then renames it).
    pub(super) fn write_persistent(&self, vm: &Vm<'a>) -> Result<(), Fatal> {
        let Some(path) = self.persistent else {
            return Ok(());
        };
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        std::fs::write(&tmp, vm.save_persistent())
            .and_then(|()| std::fs::rename(&tmp, path))
            .map_err(|e| {
                Fatal::Persistent(
                    "run-persistent-write-failed",
                    path.display().to_string(),
                    e.to_string(),
                )
            })
    }

    /// Prints to standard error that command `command` is missing its required path argument.
    pub(super) fn missing_path(&mut self, command: &str) -> Result<(), Fatal> {
        self.complain("run-missing-path", Some(&one_arg("command", command)))
    }

    /// Prints to standard error the I/O failure `e` of path `path`.
    pub(super) fn io_failure(&mut self, path: &str, e: &io::Error) -> Result<(), Fatal> {
        let mut args = one_arg("path", path);
        args.set("reason", e.to_string());
        self.complain("run-io-error", Some(&args))
    }
}
