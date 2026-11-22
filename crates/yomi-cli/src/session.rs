//! The body of `yomi run`. Takes input and output as arguments and does not touch the terminal (to make it easy to test).

mod answer;
mod files;
mod output;

pub(crate) use answer::*;
pub(crate) use output::*;

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use lingokit::{FluentArgs, Localizer};
use yomi_core::{
    CommandId, CompileOptions, Identity, Input, LoadError, LoadReport, Package, PersistentReport,
    Program, Registry, Repl, ResolvedBy, SnapshotLog, Sugar, TextTable, Value, Variable, Vm,
    VmConfig, VmError, Yield, compile_modules_with_options,
};

use yomi_std::audio::{Audio, AudioCue};
use yomi_std::image::{Image, ImageCue};
use yomi_std::screen::{Screen, ScreenCue};

use crate::diag::{diagnostic_line, error_line, load_error_message, vm_error_message};
use crate::modules::{self, SourceFile};

/// The name of the command that stops at plain lines.
const SAY: &str = "ui.say";
/// The name of the command that stops at quote lines (the speaker's identity and the body).
const SAY_AS: &str = "ui.say_as";
/// The name of the command that stops at `choice` (passes the list of options and returns the chosen number).
const CHOOSE: &str = "ui.choose";

/// The maximum number of stop points that `:back` can go back.
const ROLLBACK_LIMIT: usize = 200;

/// Settings of `yomi run` (what is decided as the host, besides the scripts).
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// The file for persistent data (`persistent var`s and passed stop points). If given, it is read at startup (new if
    /// absent) and written at each stop point and at the end.
    pub persistent: Option<PathBuf>,
    /// The random seed. If absent, made from the startup time.
    pub seed: Option<u64>,
    /// The file of the text table that replaces bodies (`--texts`). If given, it is read and set at startup.
    pub texts: Option<PathBuf>,
    /// Automatic play (`--auto`). At bodies and the stop points of `flow.wait` and `flow.wait_input`, proceeds without reading
    /// input. Only at choices and `input.*` stop points does it read a line from standard input. It also shows option IDs.
    pub auto: bool,
    /// Shows the stable IDs of options on the displayed lines (with `--auto`, shown even without this).
    pub show_ids: bool,
    /// `--lenient`: loads with `LoadPolicy::Lenient`.
    pub lenient: bool,
    /// Whether standard input is a terminal (decides whether to show prompts inside `:repl`).
    pub interactive: bool,
    /// The entry file, to collect the scripts again on `:reload`.
    pub entry: Option<PathBuf>,
    /// The search directory (`--root`), to collect the scripts again on `:reload`.
    pub root: Option<PathBuf>,
}

/// A failure that stops the run.
enum Fatal {
    /// A runtime error.
    Vm(VmError),
    /// A failure to read a save.
    Load(LoadError),
    /// A failure of the persistent data file (path and reason).
    Persistent(&'static str, String, String),
    /// An output failure (a closed pipe or the like).
    Io(io::Error),
    /// An input line could not be turned into a value (the Fluent key and the input string).
    BadInput(&'static str, String),
    /// With `--auto`, standard input ended before reading the answer to a choice.
    AutoInputEnded,
    /// Standard input ended at an event wait that has no time limit.
    EventInputEnded,
    /// The text table cannot be read or set (the line to write to standard error).
    Texts(Vec<String>),
}

impl From<VmError> for Fatal {
    fn from(e: VmError) -> Self {
        Fatal::Vm(e)
    }
}

impl From<LoadError> for Fatal {
    fn from(e: LoadError) -> Self {
        Fatal::Load(e)
    }
}

impl From<io::Error> for Fatal {
    fn from(e: io::Error) -> Self {
        Fatal::Io(e)
    }
}

/// The registry. Registers the standard packages (`flow`, `math`, `text`, `audio`, `image`, `input`) and binds stop
/// commands to plain lines, quote lines and choices.
pub(crate) fn registry() -> Registry {
    let mut r = Registry::new();
    r.stop::<(String,), ()>(SAY)
        .expect("ui.say は一度だけ登録する");
    r.stop::<(Option<Identity>, String), ()>(SAY_AS)
        .expect("ui.say_as は一度だけ登録する");
    r.stop::<(Vec<String>, Vec<Option<String>>), i64>(CHOOSE)
        .expect("ui.choose は一度だけ登録する");
    yomi_std::register_all(&mut r).expect("標準パッケージは一度だけ登録する");
    r.bind(Sugar::Plain, SAY)
        .expect("ui.say は素の行に束縛できる");
    r.bind(Sugar::Quote, SAY_AS)
        .expect("ui.say_as は引用行に束縛できる");
    r.bind(Sugar::Choice, CHOOSE)
        .expect("ui.choose は選択に束縛できる");
    r
}

/// Compiles the collected modules. Warnings are printed to standard error and it continues. If there are errors, prints
/// the diagnostics to standard error and returns `None`.
///
/// `start` is the start label (`module.label`; not warned about even if it is not called from anywhere).
pub(crate) fn compile_files<E: Write>(
    loc: &Localizer,
    reg: &Registry,
    files: &[SourceFile],
    start: &str,
    err: &mut E,
) -> Option<Program> {
    let sources = modules::sources(files);
    let options = modules::options(files, CompileOptions::default()).entry_label(start);
    match compile_modules_with_options(&sources, reg, &options) {
        Ok((p, warnings)) => {
            for d in &warnings {
                let _ = writeln!(
                    err,
                    "{}",
                    diagnostic_line(loc, &modules::path_of(files, &d.module), d)
                );
            }
            Some(p)
        }
        Err(diags) => {
            for d in &diags {
                // If writing fails, there is nothing more that can be done.
                let _ = writeln!(
                    err,
                    "{}",
                    diagnostic_line(loc, &modules::path_of(files, &d.module), d)
                );
            }
            None
        }
    }
}

/// Runs the scripts. Returns the exit code (0 for success, 1 for failure).
///
/// `files` are the collected modules (including the module of `start`'s `module.label`).
/// Diagnostics and errors are written to `err`, and bodies and responses to interactive commands to `out`.
pub fn run_script<R: BufRead, O: Write, E: Write>(
    loc: &Localizer,
    files: &[SourceFile],
    start: &str,
    options: &RunOptions,
    input: &mut R,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let reg = registry();
    let Some(program) = compile_files(loc, &reg, files, start, err) else {
        return 1;
    };
    run_compiled(loc, &reg, &program, files, start, options, input, out, err)
}

/// Runs a program already compiled from `files` ([`run_script`] without the compilation).
#[allow(clippy::too_many_arguments)]
pub(crate) fn run_compiled<R: BufRead, O: Write, E: Write>(
    loc: &Localizer,
    reg: &Registry,
    program: &Program,
    files: &[SourceFile],
    start: &str,
    options: &RunOptions,
    input: &mut R,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let start_module = start.split_once('.').map_or("", |(m, _)| m);
    let mut session = Session {
        loc,
        program,
        reg,
        start,
        seed: options.seed.unwrap_or_else(time_seed),
        persistent: options.persistent.as_deref(),
        texts_path: options.texts.as_deref(),
        texts: None,
        auto: options.auto,
        show_ids: options.show_ids,
        policy: load_policy(options.lenient),
        interactive: options.interactive,
        entry: options.entry.as_deref(),
        root: options.root.as_deref(),
        input,
        out,
        err,
    };
    match session.drive() {
        Ok(()) => 0,
        Err(fatal) => {
            if let Fatal::Texts(lines) = &fatal {
                for l in lines {
                    let _ = writeln!(err, "{l}");
                }
                return 1;
            }
            let line = match fatal {
                Fatal::Vm(e) => {
                    let file = modules::path_of(files, e.module.as_deref().unwrap_or(start_module));
                    Some(error_line(loc, &file, &vm_error_message(loc, &e)))
                }
                Fatal::Load(e) => Some(error_line(
                    loc,
                    &modules::path_of(files, start_module),
                    &load_error_message(loc, &e),
                )),
                Fatal::Persistent(key, path, reason) => {
                    let mut args = one_arg("path", &path);
                    args.set("reason", reason);
                    Some(error_line(loc, &path, &loc.fmt(key, Some(&args))))
                }
                Fatal::Io(e) => Some(error_line(
                    loc,
                    &modules::path_of(files, start_module),
                    &e.to_string(),
                )),
                Fatal::BadInput(key, text) => Some(error_line(
                    loc,
                    &modules::path_of(files, start_module),
                    &loc.fmt(key, Some(&one_arg("text", &text))),
                )),
                Fatal::AutoInputEnded => Some(error_line(
                    loc,
                    &modules::path_of(files, start_module),
                    &loc.fmt("run-auto-input-ended", None),
                )),
                Fatal::EventInputEnded => Some(error_line(
                    loc,
                    &modules::path_of(files, start_module),
                    &loc.fmt("run-event-input-ended", None),
                )),
                Fatal::Texts(_) => None,
            };
            if let Some(line) = line {
                let _ = writeln!(err, "{line}");
            }
            1
        }
    }
}

/// A random seed made from the startup time (when none was given).
pub(crate) fn time_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

/// The load policy for `--lenient`.
pub(crate) fn load_policy(lenient: bool) -> yomi_core::LoadPolicy {
    if lenient {
        yomi_core::LoadPolicy::Lenient
    } else {
        yomi_core::LoadPolicy::Strict
    }
}

/// One interactive run.
struct Session<'a, R, O, E> {
    policy: yomi_core::LoadPolicy,
    loc: &'a Localizer,
    program: &'a Program,
    reg: &'a Registry,
    start: &'a str,
    seed: u64,
    persistent: Option<&'a Path>,
    /// The table file of `--texts` (read at startup).
    texts_path: Option<&'a Path>,
    /// The text table currently set (changed by `:texts`; set again on a loaded VM too).
    texts: Option<TextTable>,
    /// Automatic play (`--auto`).
    auto: bool,
    /// Shows the IDs of options (`--show-ids`).
    show_ids: bool,
    /// Whether standard input is a terminal (prompts of `:repl`).
    interactive: bool,
    entry: Option<&'a Path>,
    root: Option<&'a Path>,
    input: &'a mut R,
    out: &'a mut O,
    err: &'a mut E,
}

/// The result of one line of input at a stop point.
enum Step {
    /// Go on to the next.
    Advance,
    /// Moved to another stop point (or the end).
    Moved(Yield),
    /// End.
    Quit,
    /// Wait for the next input at the same stop point.
    Stay,
    /// An option number was chosen (a 0-based number in the given list).
    Pick(usize),
    /// Returns the input value (the return value of `input.*`).
    Return(Value),
    /// The same stop point was rebuilt with a changed text table (`:texts`). Show it again and wait at the same stop point.
    Redraw(Yield),
}

impl<'a, R: BufRead, O: Write, E: Write> Session<'a, R, O, E> {
    /// Prints message `id` to standard output as one line.
    fn say(&mut self, id: &str, args: Option<&FluentArgs>) -> Result<(), Fatal> {
        writeln!(self.out, "{}", self.loc.fmt(id, args))?;
        Ok(())
    }

    /// Prints message `id` to standard error as one line.
    fn complain(&mut self, id: &str, args: Option<&FluentArgs>) -> Result<(), Fatal> {
        writeln!(self.err, "{}", self.loc.fmt(id, args))?;
        Ok(())
    }

    /// Runs the script until the end or an interruption. At each stop point, repeats showing, reading input and resuming.
    fn drive(&mut self) -> Result<(), Fatal> {
        let cfg = VmConfig::default()
            .start(self.start)
            .seed(self.seed)
            .load_policy(self.policy);
        let mut vm = Vm::new(self.program, self.reg, cfg)?;
        self.read_persistent(&mut vm)?;
        self.read_start_texts(&mut vm)?;
        let mut history = SnapshotLog::new(ROLLBACK_LIMIT);
        let mut current = vm.run()?;
        loop {
            // Print the cues (sound, images) added after the previous `run` before the stop point (or the end)
            self.show_cues(&vm)?;
            let (command, args) = match &current {
                Yield::Stop { command, args, .. } => (*command, args.clone()),
                _ => {
                    self.write_persistent(&vm)?;
                    self.say("run-end", None)?;
                    return Ok(());
                }
            };
            // For a choice, print the options with numbers (the VM never passes a list with 0 options).
            let expect = expectation(self.reg, command, &args);
            self.show_stop(command, &args)?;
            history.push(vm.snapshot()?);
            self.write_persistent(&vm)?;
            loop {
                // In automatic play, body and wait stop points proceed without reading input
                let step = if self.auto && expect == Expect::Next {
                    Step::Advance
                } else {
                    self.step(&mut vm, &mut history, &expect)?
                };
                match step {
                    Step::Stay => {}
                    Step::Advance => {
                        vm.resume(Input::Return(Value::Unit))?;
                        current = vm.run()?;
                        break;
                    }
                    Step::Pick(i) => {
                        vm.resume(Input::Return(Value::Int(i as i64)))?;
                        current = vm.run()?;
                        break;
                    }
                    Step::Return(v) => {
                        vm.resume(Input::Return(v))?;
                        current = vm.run()?;
                        break;
                    }
                    Step::Moved(y) => {
                        current = y;
                        break;
                    }
                    Step::Redraw(y) => {
                        if let Yield::Stop { command, args, .. } = &y {
                            self.show_stop(*command, args)?;
                        }
                    }
                    Step::Quit => return Ok(()),
                }
            }
        }
    }

    /// Prints the sound, image and screen cues before the stop point.
    fn show_cues(&mut self, vm: &Vm<'a>) -> Result<(), Fatal> {
        write_cues(
            self.loc,
            self.out,
            vm.package_state::<Audio>(),
            vm.package_state::<Image>(),
            vm.package_state::<Screen>(),
        )?;
        Ok(())
    }

    /// Prints the contents of the stop point to standard output.
    fn show_stop(&mut self, command: CommandId, args: &[Value]) -> Result<(), Fatal> {
        write_stop(
            self.loc,
            self.reg,
            self.out,
            self.auto || self.show_ids,
            command,
            args,
        )?;
        Ok(())
    }

    /// Reads one line at a stop point and handles it.
    ///
    /// At body stop points, EOF and an empty line mean "next". At a choice (`choices` is the number of options), EOF chooses
    /// number 1. Empty lines and out-of-range or non-numeric input ask for a number again.
    fn step(
        &mut self,
        vm: &mut Vm<'a>,
        history: &mut SnapshotLog,
        expect: &Expect,
    ) -> Result<Step, Fatal> {
        let mut line = String::new();
        if self.input.read_line(&mut line)? == 0 {
            // In automatic play, when the answers to choices run out, it cannot continue (do not proceed by choosing the first)
            if self.auto && matches!(expect, Expect::Choice(_)) {
                return Err(Fatal::AutoInputEnded);
            }
            return match eof_answer(expect) {
                Answer::Pick(i) => Ok(Step::Pick(i)),
                Answer::Return(v) => Ok(Step::Return(v)),
                Answer::EndOfInput => Err(Fatal::EventInputEnded),
                _ => Ok(Step::Advance),
            };
        }
        let raw = line.trim_end_matches(['\n', '\r']);
        let line = raw.trim();
        let rest = match parse_answer(expect, raw) {
            Ok(Answer::Command) => line.strip_prefix(':').unwrap_or_default(),
            Ok(Answer::Advance) => return Ok(Step::Advance),
            Ok(Answer::Pick(i)) => return Ok(Step::Pick(i)),
            Ok(Answer::Return(v)) => return Ok(Step::Return(v)),
            Ok(Answer::EndOfInput) => return Err(Fatal::EventInputEnded),
            Ok(Answer::Other) => {
                self.complain("run-unknown-input", None)?;
                return Ok(Step::Stay);
            }
            Err(e) => {
                let count = match expect {
                    Expect::Choice(ids) => ids.len(),
                    _ => 0,
                };
                return match e {
                    AnswerError::BadInt(text) => {
                        Err(Fatal::BadInput("run-input-invalid-int", text))
                    }
                    AnswerError::BadConfirm(text) => {
                        Err(Fatal::BadInput("run-input-invalid-confirm", text))
                    }
                    AnswerError::UnknownEvent(text) if self.auto => {
                        Err(Fatal::BadInput("run-auto-unknown-event", text))
                    }
                    AnswerError::EmptyEvent if self.auto => {
                        Err(Fatal::BadInput("run-auto-event-empty", String::new()))
                    }
                    AnswerError::UnknownEvent(text) => {
                        let mut args = one_arg("name", &text);
                        args.set("names", event_names(expect));
                        self.complain("run-event-unknown", Some(&args))?;
                        Ok(Step::Stay)
                    }
                    AnswerError::EmptyEvent => {
                        let args = one_arg("names", &event_names(expect));
                        self.complain("run-event-empty", Some(&args))?;
                        Ok(Step::Stay)
                    }
                    // In automatic play, when an answer does not fit, later input would be misaligned, so stop instead of continuing
                    AnswerError::UnknownId(text) if self.auto => {
                        Err(Fatal::BadInput("run-auto-unknown-id", text))
                    }
                    AnswerError::Invalid(text) if self.auto => {
                        Err(Fatal::BadInput("run-auto-invalid-answer", text))
                    }
                    AnswerError::UnknownId(text) | AnswerError::Invalid(text) => {
                        let key = if text.starts_with('#') {
                            "run-choose-unknown-id"
                        } else {
                            "run-choose-invalid"
                        };
                        let mut args = FluentArgs::new();
                        args.set("count", count as i64);
                        args.set("id", text.trim_start_matches('#').to_string());
                        self.complain(key, Some(&args))?;
                        Ok(Step::Stay)
                    }
                };
            }
        };
        let (name, arg) = match rest.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a.trim()),
            None => (rest, ""),
        };
        match name {
            "quit" => Ok(Step::Quit),
            "vars" => {
                // With multiple modules, show module variables as `module.name`
                let qualify = self.program.module_names().count() > 1;
                write_vars(self.out, &vm.variables(), qualify)?;
                Ok(Step::Stay)
            }
            "repl" => self.enter_repl(vm, history),
            "reload" => self.reload(vm, history),
            "save" => {
                if arg.is_empty() {
                    self.missing_path(":save")?;
                    return Ok(Step::Stay);
                }
                match std::fs::write(arg, vm.save()?) {
                    Ok(()) => self.say("run-saved", Some(&one_arg("path", arg)))?,
                    Err(e) => self.io_failure(arg, &e)?,
                }
                Ok(Step::Stay)
            }
            "load" => {
                if arg.is_empty() {
                    self.missing_path(":load")?;
                    return Ok(Step::Stay);
                }
                let bytes = match std::fs::read(arg) {
                    Ok(b) => b,
                    Err(e) => {
                        self.io_failure(arg, &e)?;
                        return Ok(Step::Stay);
                    }
                };
                self.restore(vm, &bytes, arg)
            }
            "texts" => {
                if arg.is_empty() {
                    self.missing_path(":texts")?;
                    return Ok(Step::Stay);
                }
                self.switch_texts(vm, arg)
            }
            "back" => {
                // The last one is the current stop point. Take out the one before it and go back.
                // `persistent var`s and passed stop points (`seen`) do not go back (rollback covers only the save state).
                if history.len() < 2 {
                    self.say("run-no-previous", None)?;
                    return Ok(Step::Stay);
                }
                history.pop_back();
                let snapshot = history.pop_back().expect("長さを確かめた");
                vm.restore(&snapshot)?;
                let y = vm.current().cloned().expect("戻した VM は停止している");
                Ok(Step::Moved(y))
            }
            _ => {
                let args = one_arg("command", &format!(":{name}"));
                // The old name `:lang` points to `:texts`
                let key = if name == "lang" {
                    "run-unknown-command-lang"
                } else {
                    "run-unknown-command"
                };
                self.complain(key, Some(&args))?;
                Ok(Step::Stay)
            }
        }
    }

    /// `:repl`. Enters the REPL on a copy of the environment of the current stop point, and returns to the same stop point
    /// when done.
    ///
    /// If variables were applied with `:apply`, the rebuilt stop point is shown again (the same as after `:texts`).
    fn enter_repl(&mut self, vm: &mut Vm<'a>, history: &mut SnapshotLog) -> Result<Step, Fatal> {
        let mut repl = Repl::attach(vm)?;
        let module = vm.module_name().to_string();
        self.say("repl-enter", None)?;
        let mut driver = crate::repl::Driver {
            loc: self.loc,
            reg: self.reg,
            show_ids: self.auto || self.show_ids,
            prompt: self.interactive,
            qualify: self.program.module_names().count() > 1,
            current: Some(&module),
            input: &mut *self.input,
            out: &mut *self.out,
            err: &mut *self.err,
        };
        let end = driver.run(&mut repl, Some((vm, history)))?;
        match end {
            crate::repl::End::Quit => Ok(Step::Quit),
            crate::repl::End::Exit { applied: true } => {
                let y = vm.current().cloned().expect("反映した VM は停止している");
                Ok(Step::Redraw(y))
            }
            crate::repl::End::Exit { applied: false } => Ok(Step::Stay),
        }
    }
}
