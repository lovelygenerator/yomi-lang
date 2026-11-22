//! The body of `yomi repl` and of `:repl` in `yomi run` (spec/host/07-repl.md). Takes input and output as arguments and
//! does not touch the terminal.
//!
//! Classifying input (complete, incomplete, invalid) and running expressions, statements, declarations and script lines
//! are done by the core's [`Repl`] and [`completeness`]. This handles only reading lines, showing prompts, interactive
//! commands, showing stop points and taking input at them, and showing errors. The rules for showing stop points and
//! taking input are the same as `yomi run` (`session.rs`).

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use lingokit::{FluentArgs, Localizer};
use yomi_core::{
    ApplyError, ApplyInvalid, Completeness, Declared, Diagnostic, Input, Registry, Repl, ReplYield,
    SnapshotLog, Ty, Value, Variable, Vm, VmConfig, VmError, VmErrorKind, completeness,
};
use yomi_std::audio::Audio;
use yomi_std::image::Image;
use yomi_std::screen::Screen;

use crate::diag::{
    diagnostic_line, error_line, load_kind_message, vm_error_message, vm_kind_message,
};
use crate::modules::SourceFile;
use crate::session::{
    Answer, AnswerError, compile_files, eof_answer, expectation, one_arg, parse_answer, registry,
    say, time_seed, write_cues, write_stop, write_vars,
};

/// The "file" name of REPL input (used to show diagnostics and errors).
const REPL_FILE: &str = "<repl>";
/// The name of the module that holds REPL variables (the `module` of `Repl::variables`).
const REPL_MODULE: &str = "<repl>";
/// The prompt for the start of input.
const PROMPT: &str = "yomi> ";
/// The prompt for continuing input.
const PROMPT_MORE: &str = "...> ";
/// The list of available commands, added for an unknown command.
const COMMAND_LIST: &str = ":help :vars :defs :type :apply :reset :exit :quit :cancel :abort";

/// How the REPL ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    /// `:exit` or EOF. `applied` tells whether `:apply` applied variables even once.
    Exit { applied: bool },
    /// `:quit`. Stops the whole run.
    Quit,
}

/// The result of a command typed in the middle of a stop point.
enum StopCmd {
    /// The stop point was resumed (the caller runs the continuation).
    Resumed,
    /// Wait for the next input at the same stop point (a command was handled).
    Stay,
    /// The input was aborted (`:abort`).
    Abort,
    /// Stop the whole run.
    Quit,
}

/// The flow after finishing running one input.
enum After {
    /// To the next input.
    Continue,
    /// `:quit`.
    Quit,
}

/// The input and output of the REPL, and display settings.
pub(crate) struct Driver<'d, R, O, E> {
    pub loc: &'d Localizer,
    pub reg: &'d Registry,
    /// Whether to show the stable IDs of options.
    pub show_ids: bool,
    /// Whether to show prompts (when standard input is a terminal).
    pub prompt: bool,
    /// Whether to show the script's module variables as `module.name` (when there are multiple modules).
    pub qualify: bool,
    /// The current module (`:type` shows this module's types without the module name).
    pub current: Option<&'d str>,
    pub input: &'d mut R,
    pub out: &'d mut O,
    pub err: &'d mut E,
}

/// The stopped script VM and its snapshot record, borrowed when entering `:repl` in the middle of `yomi run`. `None` for a
/// standalone `yomi repl`.
type Attached<'x, 'a> = Option<(&'x mut Vm<'a>, &'x mut SnapshotLog)>;

impl<R: BufRead, O: Write, E: Write> Driver<'_, R, O, E> {
    /// Prints message `id` to standard output as one line.
    fn say(&mut self, id: &str, args: Option<&FluentArgs>) -> io::Result<()> {
        say(self.loc, self.out, id, args)
    }

    /// Prints message `id` to standard error as one line.
    fn complain(&mut self, id: &str, args: Option<&FluentArgs>) -> io::Result<()> {
        writeln!(self.err, "{}", self.loc.fmt(id, args))
    }

    /// Reads one line (without the newline). EOF is `None`.
    fn read_line(&mut self) -> io::Result<Option<String>> {
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        Ok(Some(line.trim_end_matches(['\n', '\r']).to_string()))
    }

    /// Only when showing prompts is enabled, prints `text` and flushes standard output.
    fn show_prompt(&mut self, text: &str) -> io::Result<()> {
        if self.prompt {
            write!(self.out, "{text}")?;
            self.out.flush()?;
        }
        Ok(())
    }

    /// Prints a diagnostic to standard error as `<repl>:line:col: severity: text`.
    fn report_diags(&mut self, diags: &[Diagnostic]) -> io::Result<()> {
        for d in diags {
            writeln!(self.err, "{}", diagnostic_line(self.loc, REPL_FILE, d))?;
        }
        Ok(())
    }

    /// Prints a runtime error to standard error as `<repl>: error: text`.
    ///
    /// `multiline` tells whether the input has multiple lines (the line within the input is shown only for multiple lines).
    fn report_runtime(&mut self, e: &VmError, multiline: bool) -> io::Result<()> {
        let message = repl_error_message(self.loc, e, multiline);
        writeln!(self.err, "{}", error_line(self.loc, REPL_FILE, &message))
    }

    /// Repeats reading input and running it until `:exit`, `:quit` or EOF.
    ///
    /// `attached` is the stopped VM when entered with `:repl`, and the snapshots for `:back` (used by `:apply`).
    pub(crate) fn run<'a>(
        &mut self,
        repl: &mut Repl<'a>,
        mut attached: Attached<'_, 'a>,
    ) -> io::Result<End> {
        let mut applied = false;
        loop {
            self.show_prompt(PROMPT)?;
            let Some(first) = self.read_line()? else {
                return Ok(End::Exit { applied });
            };
            if first.trim().starts_with(':') {
                match self.command(repl, &mut attached, first.trim(), &mut applied)? {
                    Flow::Go => continue,
                    Flow::Exit => return Ok(End::Exit { applied }),
                    Flow::Quit => return Ok(End::Quit),
                }
            }
            // Collect input. When the syntax is complete, commit it right away (empty lines do not commit)
            let mut text = first;
            let mut eof = false;
            let mut cancelled = false;
            let mut invalid = false;
            loop {
                match completeness(&text) {
                    Completeness::Incomplete => {
                        self.show_prompt(PROMPT_MORE)?;
                        match self.read_line()? {
                            None => {
                                eof = true;
                                break;
                            }
                            Some(l) if l.trim() == ":cancel" => {
                                cancelled = true;
                                break;
                            }
                            Some(l) => {
                                text.push('\n');
                                text.push_str(&l);
                            }
                        }
                    }
                    Completeness::Invalid(diags) => {
                        self.report_diags(&diags)?;
                        invalid = true;
                        break;
                    }
                    _ => break,
                }
            }
            if cancelled || invalid {
                continue;
            }
            if let After::Quit = self.execute_input(repl, &text)? {
                return Ok(End::Quit);
            }
            if eof {
                return Ok(End::Exit { applied });
            }
        }
    }

    /// Type-checks and runs input that is complete (or committed at EOF).
    fn execute_input(&mut self, repl: &mut Repl<'_>, text: &str) -> io::Result<After> {
        let multiline = text.contains('\n');
        let submitted = match repl.submit(text) {
            Ok(s) => s,
            Err(diags) => {
                self.report_diags(&diags)?;
                return Ok(After::Continue);
            }
        };
        self.report_diags(&submitted.warnings)?;
        loop {
            match repl.run() {
                Err(e) => {
                    self.report_runtime(&e, multiline)?;
                    return Ok(After::Continue);
                }
                Ok(y) => {
                    // Print the cues (sound, images) added by the previous run before the stop point (or the end)
                    write_cues(
                        self.loc,
                        self.out,
                        repl.package_state::<Audio>(),
                        repl.package_state::<Image>(),
                        repl.package_state::<Screen>(),
                    )?;
                    match y {
                        ReplYield::Stop { command, args, .. } => {
                            match self.stop(repl, command, &args, multiline)? {
                                StopCmd::Resumed => continue,
                                StopCmd::Stay | StopCmd::Abort => return Ok(After::Continue),
                                StopCmd::Quit => return Ok(After::Quit),
                            }
                        }
                        ReplYield::Done {
                            value,
                            declared,
                            ended,
                        } => {
                            if let Some((v, _)) = value {
                                writeln!(self.out, "{}", v.to_repl_text())?;
                            }
                            for d in &declared {
                                if let Declared::Let { name, value, .. } = d {
                                    writeln!(self.out, "{name} = {}", value.to_repl_text())?;
                                }
                            }
                            if ended {
                                self.say("run-end", None)?;
                            }
                            return Ok(After::Continue);
                        }
                        _ => return Ok(After::Continue),
                    }
                }
            }
        }
    }

    /// Shows a stop point, reads the answer and resumes. If resumed, `Resumed` (the caller `run`s the continuation).
    ///
    /// On input errors and runtime errors, aborts the input and returns `Abort`. For invalid choices, asks again at the same
    /// stop point.
    fn stop(
        &mut self,
        repl: &mut Repl<'_>,
        command: yomi_core::CommandId,
        args: &[Value],
        multiline: bool,
    ) -> io::Result<StopCmd> {
        let expect = expectation(self.reg, command, args);
        write_stop(self.loc, self.reg, self.out, self.show_ids, command, args)?;
        loop {
            let answer = match self.read_line()? {
                None => match eof_answer(&expect) {
                    Answer::EndOfInput => {
                        return self.bad_input(repl, "run-event-input-ended", "");
                    }
                    other => other,
                },
                Some(line) => match parse_answer(&expect, &line) {
                    Ok(Answer::Command) => {
                        match self.stop_command(repl, line.trim())? {
                            StopCmd::Stay => continue,
                            other => return Ok(other),
                        };
                    }
                    Ok(Answer::Other) => {
                        self.complain("run-unknown-input", None)?;
                        continue;
                    }
                    Ok(a) => a,
                    Err(AnswerError::BadInt(text)) => {
                        return self.bad_input(repl, "run-input-invalid-int", &text);
                    }
                    Err(AnswerError::BadConfirm(text)) => {
                        return self.bad_input(repl, "run-input-invalid-confirm", &text);
                    }
                    Err(AnswerError::UnknownEvent(text)) => {
                        let mut a = one_arg("name", &text);
                        a.set("names", crate::session::event_names(&expect));
                        self.complain("run-event-unknown", Some(&a))?;
                        continue;
                    }
                    Err(AnswerError::EmptyEvent) => {
                        let a = one_arg("names", &crate::session::event_names(&expect));
                        self.complain("run-event-empty", Some(&a))?;
                        continue;
                    }
                    Err(AnswerError::UnknownId(text) | AnswerError::Invalid(text)) => {
                        let count = match &expect {
                            crate::session::Expect::Choice(ids) => ids.len(),
                            _ => 0,
                        };
                        let key = if text.starts_with('#') {
                            "run-choose-unknown-id"
                        } else {
                            "run-choose-invalid"
                        };
                        let mut a = FluentArgs::new();
                        a.set("count", count as i64);
                        a.set("id", text.trim_start_matches('#').to_string());
                        self.complain(key, Some(&a))?;
                        continue;
                    }
                },
            };
            let value = match answer {
                Answer::Pick(i) => Value::Int(i as i64),
                Answer::Return(v) => v,
                _ => Value::Unit,
            };
            return match repl.resume(Input::Return(value)) {
                Ok(()) => Ok(StopCmd::Resumed),
                Err(e) => {
                    repl.abort();
                    self.report_runtime(&e, multiline)?;
                    Ok(StopCmd::Abort)
                }
            };
        }
    }

    /// An input error (not an integer, not y/n). Aborts the input.
    fn bad_input(&mut self, repl: &mut Repl<'_>, key: &str, text: &str) -> io::Result<StopCmd> {
        repl.abort();
        let message = self.loc.fmt(key, Some(&one_arg("text", text)));
        writeln!(self.err, "{}", error_line(self.loc, REPL_FILE, &message))?;
        Ok(StopCmd::Abort)
    }

    /// A command typed in the middle of a stop point. Only `:vars`, `:abort`, `:quit` and `:help` can be used.
    fn stop_command(&mut self, repl: &mut Repl<'_>, line: &str) -> io::Result<StopCmd> {
        let (name, _) = split_command(line);
        match name {
            "quit" => Ok(StopCmd::Quit),
            "abort" => {
                repl.abort();
                Ok(StopCmd::Abort)
            }
            "vars" => {
                self.show_vars(repl)?;
                Ok(StopCmd::Stay)
            }
            "help" => {
                self.show_help()?;
                Ok(StopCmd::Stay)
            }
            "apply" | "reset" | "exit" | "cancel" | "defs" | "type" => {
                let a = one_arg("command", &format!(":{name}"));
                self.complain("repl-not-at-stop", Some(&a))?;
                Ok(StopCmd::Stay)
            }
            _ => {
                self.unknown_or_unavailable(name)?;
                Ok(StopCmd::Stay)
            }
        }
    }

    /// A command typed at the prompt.
    fn command<'a, 'x>(
        &mut self,
        repl: &mut Repl<'a>,
        attached: &mut Attached<'x, 'a>,
        line: &str,
        applied: &mut bool,
    ) -> io::Result<Flow> {
        let (name, arg) = split_command(line);
        match name {
            "exit" => return Ok(Flow::Exit),
            "quit" => return Ok(Flow::Quit),
            // There is no partially written input, so nothing happens
            "cancel" => {}
            "help" => self.show_help()?,
            "vars" => self.show_vars(repl)?,
            "defs" => {
                for d in repl.defs() {
                    writeln!(self.out, "{d}")?;
                }
            }
            "type" => {
                if arg.is_empty() {
                    self.complain("repl-missing-arg", Some(&one_arg("command", ":type")))?;
                } else {
                    match repl.type_of(arg) {
                        Ok(ty) => writeln!(self.out, "{}", type_name(&ty, self.current))?,
                        Err(diags) => self.report_diags(&diags)?,
                    }
                }
            }
            "reset" => {
                if let Err(e) = repl.reset() {
                    self.report_runtime(&e, false)?;
                }
            }
            "apply" => self.apply(repl, attached, applied)?,
            "abort" => self.complain("repl-abort-not-stopped", None)?,
            _ => self.unknown_or_unavailable(name)?,
        }
        Ok(Flow::Go)
    }

    /// Commands that cannot be used inside the REPL, such as `:save`, and unknown commands.
    fn unknown_or_unavailable(&mut self, name: &str) -> io::Result<()> {
        let mut a = one_arg("command", &format!(":{name}"));
        match name {
            "save" | "load" | "back" | "texts" => self.complain("repl-not-here", Some(&a)),
            "repl" => self.complain("repl-already", None),
            _ => {
                a.set("list", COMMAND_LIST);
                self.complain("repl-unknown-command", Some(&a))
            }
        }
    }

    /// Prints the list of interactive commands for `:help`.
    fn show_help(&mut self) -> io::Result<()> {
        for key in [
            "repl-help-help",
            "repl-help-vars",
            "repl-help-defs",
            "repl-help-type",
            "repl-help-apply",
            "repl-help-reset",
            "repl-help-exit",
            "repl-help-quit",
            "repl-help-cancel",
            "repl-help-abort",
        ] {
            self.say(key, None)?;
        }
        Ok(())
    }

    /// `:vars`. The script's variables (in the same order as `:vars` of `yomi run`), a separator line, and REPL variables.
    fn show_vars(&mut self, repl: &Repl<'_>) -> io::Result<()> {
        let (mine, script): (Vec<Variable>, Vec<Variable>) = repl
            .variables()
            .into_iter()
            .partition(|v| v.module.as_deref() == Some(REPL_MODULE));
        write_vars(self.out, &script, self.qualify)?;
        if !mine.is_empty() {
            self.say("repl-vars-separator", None)?;
            write_vars(self.out, &mine, false)?;
        }
        Ok(())
    }

    /// `:apply`. Writes variables changed in the copy back to the stopped script (section 7.3.6 of `spec/host/07-repl.md`).
    fn apply<'a, 'x>(
        &mut self,
        repl: &mut Repl<'a>,
        attached: &mut Attached<'x, 'a>,
        applied: &mut bool,
    ) -> io::Result<()> {
        let Some((vm, history)) = attached.as_mut() else {
            return self.complain("repl-apply-unavailable", None);
        };
        match repl.apply(vm) {
            Ok(done) => {
                if !done.skipped.is_empty() {
                    let names: Vec<String> = done.skipped.iter().map(ToString::to_string).collect();
                    self.complain(
                        "repl-apply-skipped",
                        Some(&one_arg("names", &names.join(", "))),
                    )?;
                }
                if done.variables.is_empty() {
                    return self.say("repl-apply-nothing", None);
                }
                // Print the applied variables as `name = value`
                write_vars(self.out, &done.variables, self.qualify)?;
                *applied = true;
                // Of the snapshots for rollback, retake the one for the current stop point (the last) with the applied state
                match vm.snapshot() {
                    Ok(snapshot) => {
                        history.pop_back();
                        history.push(snapshot);
                    }
                    Err(e) => self.report_runtime(&e, false)?,
                }
                Ok(())
            }
            Err(ApplyError::Invalid { name, reason }) => {
                let reason_key = match reason {
                    ApplyInvalid::Type => "repl-apply-reason-type",
                    ApplyInvalid::ValueTooLarge => "repl-apply-reason-too-large",
                    ApplyInvalid::MemoryLimit => "repl-apply-reason-memory",
                    _ => "repl-apply-reason-not-savable",
                };
                let mut a = one_arg("name", &name.to_string());
                a.set("reason", self.loc.fmt(reason_key, None));
                self.complain("repl-apply-invalid", Some(&a))
            }
            Err(ApplyError::StopArgs(e)) => {
                let message = repl_error_message(self.loc, &e, false);
                let mut a = FluentArgs::new();
                a.set("message", message);
                self.complain("repl-apply-stop-args", Some(&a))
            }
            Err(_) => self.complain("repl-apply-unavailable", None),
        }
    }
}

/// Turns a type name into how it is written in scripts. Types of the current module and the REPL are shown without the
/// module name.
fn type_name(ty: &Ty, current: Option<&str>) -> String {
    match ty {
        Ty::List(t) => format!("list<{}>", type_name(t, current)),
        Ty::Map(k, v) => format!("map<{}, {}>", type_name(k, current), type_name(v, current)),
        Ty::Option(t) => format!("Option<{}>", type_name(t, current)),
        Ty::Struct(n) | Ty::Enum(n) => match n.rsplit_once('.') {
            Some((m, short)) if m == REPL_MODULE || Some(m) == current => short.to_string(),
            _ => n.clone(),
        },
        other => other.name(),
    }
}

/// The flow of a command.
enum Flow {
    Go,
    Exit,
    Quit,
}

/// Splits `:name args` into the name and the arguments (without surrounding whitespace).
fn split_command(line: &str) -> (&str, &str) {
    let rest = line.trim().strip_prefix(':').unwrap_or(line);
    match rest.split_once(char::is_whitespace) {
        Some((n, a)) => (n, a.trim()),
        None => (rest, ""),
    }
}

/// The text of a REPL runtime error. When it happened inside REPL input, no label is attached, and the line of the input
/// is attached only for multiple lines.
fn repl_error_message(loc: &Localizer, e: &VmError, multiline: bool) -> String {
    if e.module.as_deref() == Some(REPL_MODULE) {
        let message = vm_kind_message(loc, &e.kind, (None, None), None);
        return match e.line.filter(|_| multiline) {
            Some(line) => {
                let mut a = FluentArgs::new();
                a.set("message", message);
                a.set("line", i64::from(line));
                loc.fmt("repl-located-line", Some(&a))
            }
            None => message,
        };
    }
    vm_error_message(loc, e)
}

/// Settings of `yomi repl`.
#[derive(Debug, Clone, Default)]
pub struct ReplOptions {
    /// The file of persistent data (only read).
    pub persistent: Option<PathBuf>,
    /// The random seed. If absent, made from the startup time.
    pub seed: Option<u64>,
    /// `--lenient`: loads persistent data with `LoadPolicy::Lenient`.
    pub lenient: bool,
    /// Whether standard input is a terminal (whether to show prompts).
    pub interactive: bool,
}

/// `yomi repl`. If there is a script (the collected modules and the name of the entry file's module), runs on the
/// environment at its start point; otherwise in an empty environment. Returns the exit code (0 for success, 1 for
/// failure).
pub fn run_repl<R: BufRead, O: Write, E: Write>(
    loc: &Localizer,
    script: Option<(&[SourceFile], &str)>,
    options: &ReplOptions,
    input: &mut R,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let reg = registry();
    let start = script.map(|(_, module)| format!("{module}.start"));
    let program = match script {
        Some((files, _)) => {
            let Some(p) = compile_files(loc, &reg, files, start.as_deref().unwrap_or(""), err)
            else {
                return 1;
            };
            Some(p)
        }
        None => None,
    };
    let mut cfg = VmConfig::default()
        .seed(options.seed.unwrap_or_else(time_seed))
        .load_policy(crate::session::load_policy(options.lenient));
    if let Some(start) = &start {
        cfg = cfg.start(start);
    }
    // Persistent data is only read (new if the file does not exist)
    if let Some(path) = &options.persistent {
        match std::fs::read(path) {
            Ok(bytes) => cfg = cfg.persistent_data(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                let shown = path.display().to_string();
                let mut a = one_arg("path", &shown);
                a.set("reason", e.to_string());
                let message = loc.fmt("run-persistent-read-failed", Some(&a));
                let _ = writeln!(err, "{}", error_line(loc, &shown, &message));
                return 1;
            }
        }
    }
    let made = match &program {
        Some(p) => Repl::from_program(p, &reg, cfg),
        None => Repl::empty(&reg, cfg),
    };
    let mut repl = match made {
        Ok(r) => r,
        Err(e) => {
            let shown = options
                .persistent
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let message = match &e.kind {
                VmErrorKind::PersistentData(load) => {
                    let mut a = one_arg("path", &shown);
                    a.set("reason", load_kind_message(loc, load));
                    loc.fmt("run-persistent-read-failed", Some(&a))
                }
                _ => vm_error_message(loc, &e),
            };
            let _ = writeln!(err, "{}", error_line(loc, &shown, &message));
            return 1;
        }
    };
    let mut driver = Driver {
        loc,
        reg: &reg,
        show_ids: false,
        prompt: options.interactive,
        qualify: program
            .as_ref()
            .is_some_and(|p| p.module_names().count() > 1),
        current: script.map(|(_, module)| module),
        input,
        out,
        err,
    };
    match driver.run(&mut repl, None) {
        Ok(_) => 0,
        // Cannot write to standard output or standard error (a closed pipe or the like)
        Err(_) => 1,
    }
}
