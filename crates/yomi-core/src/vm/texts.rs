//! Text replacement in the VM (localization; `spec/host/05-texts.md`).
//!
//! When the VM reaches a stop point, it holds the original arguments and the evaluated values of `${expr}` (`cur_src`).
//! The text table (`texts`) is used only for stop notifications (`Yield`) and for re-rendering the history
//! ([`Vm::render`]). It has nothing to do with position, variables, saves, history, passed stop points or the random
//! generator (the language is not included in saves).

use super::{StopRecord, StopSrc, Vm, VmError, VmErrorKind, Yield, label_path};
use crate::program::StopKind;
use crate::texts::{StopCtx, TextIssue, TextTable, localize_args};
use crate::value::Value;

impl<'a> Vm<'a> {
    /// Sets the text table that replaces bodies. `None` goes back to the originals.
    ///
    /// Can be called at any time. Replacement changes from the next stop point (the [`Yield`] of the current stop point does
    /// not change; to rebuild it, use [`Vm::refresh_current`]). If the `n` of `${n}` in a translation exceeds the number of
    /// `${expr}` in the original, it is an error (all number errors; the table is not set). Translations with keys not in the
    /// scripts are not errors here ([`TextTable::check`] warns about them).
    pub fn set_texts(&mut self, texts: Option<TextTable>) -> Result<(), Vec<TextIssue>> {
        if let Some(t) = &texts {
            let errors = t.number_errors(&self.program);
            if !errors.is_empty() {
                return Err(errors);
            }
        }
        self.texts = texts;
        Ok(())
    }

    /// The text table currently set.
    pub fn texts(&self) -> Option<&TextTable> {
        self.texts.as_ref()
    }

    /// Returns the stop notification of the current stop point with its bodies (plain lines, quote lines, option texts)
    /// rebuilt with the current text table.
    ///
    /// Can only be called while stopped (otherwise `NotStopped`). Uses the originals and the evaluated `${expr}` values held by
    /// the VM, without re-evaluating expressions (the same values even if variables changed). Position, variables, passed stop
    /// points, history and the random generator do not change, and nothing is added to the history. For stop points without
    /// bodies, returns the same notification. The returned notification becomes [`Vm::current`] from then on.
    pub fn refresh_current(&mut self) -> Result<Yield, VmError> {
        let super::State::Stopped { ordinal } = self.state else {
            return Err(self.misuse(VmErrorKind::NotStopped));
        };
        let (Some(Yield::Stop { command, id, .. }), Some(src)) = (&self.current, &self.cur_src)
        else {
            return Err(self.misuse(VmErrorKind::NotStopped));
        };
        let y = Yield::Stop {
            command: *command,
            args: self.localize(ordinal, id.as_deref(), &src.args, &src.values),
            id: id.clone(),
        };
        self.current = Some(y.clone());
        Ok(y)
    }

    /// Rebuilds the arguments of one history entry with the current text table (bodies become translations; those not in the
    /// table stay as the original). The history holds the originals and evaluated values, so it also works with history
    /// after a load.
    pub fn render(&self, record: &StopRecord) -> Vec<Value> {
        // The translatable arguments of a command are looked up in the scripts (the history only has the kind and key)
        let text_arg = match (record.kind, record.key.as_deref()) {
            (StopKind::Command, Some(key)) => key
                .split_once('#')
                .and_then(|(_, id)| self.program.command_text_arg(&record.label, id)),
            _ => None,
        };
        let ctx = StopCtx {
            program: &self.program,
            kind: record.kind,
            label: &record.label,
            key: record.key.as_deref(),
            text_arg,
        };
        localize_args(self.texts.as_ref(), &ctx, &record.args, &record.values)
    }

    /// Returns the body of one history entry (a plain line or quote line) as a string rebuilt with the current text table.
    /// `None` for stop points without exactly one body (`@` commands, choices; for a choice, [`Vm::render`] gives the `list`
    /// of option texts).
    pub fn history_text(&self, record: &StopRecord) -> Option<String> {
        let at = match record.kind {
            StopKind::Plain => 0,
            StopKind::Quote => 1,
            _ => return None,
        };
        match self.render(record).into_iter().nth(at)? {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        }
    }

    /// Rebuilds the original arguments with the current text table (the `ordinal`-th stop point of the current label).
    pub(super) fn localize(
        &self,
        ordinal: u32,
        id: Option<&str>,
        args: &[Value],
        values: &[Vec<String>],
    ) -> Vec<Value> {
        let label = label_path(&self.program, self.label);
        let stop = &self.program.labels[self.label].stops[ordinal as usize];
        let key = id.map(|id| format!("{label}#{id}"));
        let ctx = StopCtx {
            program: &self.program,
            kind: stop.kind,
            label: &label,
            key: key.as_deref(),
            text_arg: stop.text_arg,
        };
        localize_args(self.texts.as_ref(), &ctx, args, values)
    }

    /// Reached a stop point. Remembers the original notification `orig` and the current `${expr}` values, records it in the
    /// history, and returns the replaced notification after making it `current`.
    pub(super) fn arrive(&mut self, orig: Yield, ordinal: u32) -> Yield {
        let Yield::Stop { command, args, id } = &orig else {
            return orig;
        };
        let src = StopSrc {
            args: args.clone(),
            values: self.cap.clone(),
        };
        let y = Yield::Stop {
            command: *command,
            args: self.localize(ordinal, id.as_deref(), &src.args, &src.values),
            id: id.clone(),
        };
        self.cur_src = Some(src);
        self.record_current(&orig, ordinal);
        self.current = Some(y.clone());
        y
    }

    /// Makes the original notification rebuilt by a load or by reading persistent data the current stop point (not recorded
    /// in the history). `cap` are the `${expr}` values when it was rebuilt.
    pub(super) fn install_current(&mut self, orig: Yield, ordinal: u32) {
        let Yield::Stop { command, args, id } = &orig else {
            self.current = Some(orig);
            return;
        };
        let src = StopSrc {
            args: args.clone(),
            values: self.cap.clone(),
        };
        let y = Yield::Stop {
            command: *command,
            args: self.localize(ordinal, id.as_deref(), &src.args, &src.values),
            id: id.clone(),
        };
        self.cur_src = Some(src);
        self.current = Some(y);
    }
}
