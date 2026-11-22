//! `yomi debug`: runs a script a source line at a time, with breakpoints and inspection (`spec/host/06-cli.md`, 6.18;
//! `spec/host/09-debug.md`).

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use lingokit::{FluentArgs, Localizer};
use yomi_core::{
    DebugYield, Debugger, FrameInfo, Input, Program, Registry, StepMode, Value, Variable, Vm,
    VmConfig, VmError, Yield,
};
use yomi_std::audio::Audio;
use yomi_std::image::Image;
use yomi_std::screen::Screen;

use crate::diag::{error_line, vm_error_message};
use crate::modules::{self, SourceFile};
use crate::session::{
    self, Answer, AnswerError, Expect, eof_answer, event_names, expectation, one_arg, parse_answer,
    write_cues, write_vars,
};
use crate::texts;

/// The prompt shown when standard input is a terminal.
const PROMPT: &str = "(yomi) ";

/// How many lines `list` shows on each side of the current line.
const LIST_CONTEXT: u32 = 5;

/// Settings of `yomi debug`.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub seed: Option<u64>,
    pub texts: Option<PathBuf>,
    /// Whether standard input is a terminal (decides whether to show the prompt).
    pub interactive: bool,
}

/// A command that runs the script on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Motion {
    Step,
    Next,
    Finish,
    Continue,
}

impl Motion {
    fn mode(self) -> StepMode {
        match self {
            Motion::Step => StepMode::In,
            Motion::Next => StepMode::Over,
            Motion::Finish => StepMode::Out,
            Motion::Continue => StepMode::Continue,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Motion(Motion),
    Break,
    Delete,
    Breaks,
    Bt,
    Frame,
    Up,
    Down,
    Vars,
    Print,
    List,
    Quit,
}

fn parse_command(name: &str) -> Option<Cmd> {
    Some(match name {
        "step" | "s" => Cmd::Motion(Motion::Step),
        "next" | "n" => Cmd::Motion(Motion::Next),
        "finish" | "fin" => Cmd::Motion(Motion::Finish),
        "continue" | "c" => Cmd::Motion(Motion::Continue),
        "break" | "b" => Cmd::Break,
        "delete" | "d" => Cmd::Delete,
        "breaks" => Cmd::Breaks,
        "bt" | "where" => Cmd::Bt,
        "frame" | "f" => Cmd::Frame,
        "up" => Cmd::Up,
        "down" => Cmd::Down,
        "vars" | "v" => Cmd::Vars,
        "print" | "p" => Cmd::Print,
        "list" | "l" => Cmd::List,
        "quit" | "q" => Cmd::Quit,
        _ => return None,
    })
}

/// What a command asks of the loop that reads it.
enum Flow {
    /// Read the next line.
    Stay,
    /// Run the script on (only at a pause; at a stop point the mode is set and the answer is awaited).
    Go,
    Quit,
}

/// A failure that stops the session.
enum Fatal {
    Vm(VmError),
    Io(io::Error),
    /// A line to write to standard error as it is.
    Lines(Vec<String>),
    /// An input line that is not a valid answer (the Fluent key and the text).
    BadInput(&'static str, String),
    EventInputEnded,
}

impl From<VmError> for Fatal {
    fn from(e: VmError) -> Self {
        Fatal::Vm(e)
    }
}

impl From<io::Error> for Fatal {
    fn from(e: io::Error) -> Self {
        Fatal::Io(e)
    }
}

/// The answer read at a stop point.
enum Reply {
    Resume(Value),
    Quit,
}

struct Breakpoint {
    number: u32,
    module: String,
    line: u32,
}

/// Runs the scripts under the debugger. Returns the exit code.
pub fn run_script<R: BufRead, O: Write, E: Write>(
    loc: &Localizer,
    files: &[SourceFile],
    start: &str,
    options: &Options,
    input: &mut R,
    out: &mut O,
    err: &mut E,
) -> i32 {
    let reg = session::registry();
    let Some(program) = session::compile_files(loc, &reg, files, start, err) else {
        return 1;
    };
    let mut session = Session {
        loc,
        files,
        program: &program,
        reg: &reg,
        interactive: options.interactive,
        breakpoints: Vec::new(),
        next_number: 1,
        frame: 0,
        last_motion: Motion::Step,
        last_pause: None,
        input,
        out,
        err,
    };
    let start_module = start.split_once('.').map_or("", |(m, _)| m);
    match session.drive(start, options) {
        Ok(()) => 0,
        Err(fatal) => {
            let line = match fatal {
                Fatal::Lines(lines) => {
                    for l in lines {
                        let _ = writeln!(err, "{l}");
                    }
                    return 1;
                }
                Fatal::Vm(e) => {
                    let file = modules::path_of(files, e.module.as_deref().unwrap_or(start_module));
                    error_line(loc, &file, &vm_error_message(loc, &e))
                }
                Fatal::Io(e) => {
                    error_line(loc, &modules::path_of(files, start_module), &e.to_string())
                }
                Fatal::BadInput(key, text) => error_line(
                    loc,
                    &modules::path_of(files, start_module),
                    &loc.fmt(key, Some(&one_arg("text", &text))),
                ),
                Fatal::EventInputEnded => error_line(
                    loc,
                    &modules::path_of(files, start_module),
                    &loc.fmt("run-event-input-ended", None),
                ),
            };
            let _ = writeln!(err, "{line}");
            1
        }
    }
}

struct Session<'a, R, O, E> {
    loc: &'a Localizer,
    files: &'a [SourceFile],
    program: &'a Program,
    reg: &'a Registry,
    interactive: bool,
    breakpoints: Vec<Breakpoint>,
    next_number: u32,
    /// The frame the inspection commands look at. 0 is the innermost; reset whenever the script runs.
    frame: usize,
    /// What an empty line repeats.
    last_motion: Motion,
    /// Where the last shown pause was (module, line, name, depth), to skip the second pause on the line a call returns to.
    last_pause: Option<(String, u32, String, usize)>,
    input: &'a mut R,
    out: &'a mut O,
    err: &'a mut E,
}

impl<'a, R: BufRead, O: Write, E: Write> Session<'a, R, O, E> {
    fn say(&mut self, id: &str, args: Option<&FluentArgs>) -> Result<(), Fatal> {
        writeln!(self.out, "{}", self.loc.fmt(id, args))?;
        Ok(())
    }

    fn complain(&mut self, id: &str, args: Option<&FluentArgs>) -> Result<(), Fatal> {
        writeln!(self.err, "{}", self.loc.fmt(id, args))?;
        Ok(())
    }

    fn drive(&mut self, start: &str, options: &Options) -> Result<(), Fatal> {
        let cfg = VmConfig::default()
            .start(start)
            .seed(options.seed.unwrap_or_else(session::time_seed));
        let mut vm = Vm::new(self.program, self.reg, cfg)?;
        if let Some(path) = &options.texts {
            self.set_texts(&mut vm, &path.display().to_string())?;
        }
        let mut dbg = Debugger::new();
        dbg.set_mode(StepMode::In);
        loop {
            self.frame = 0;
            match vm.run_debug(&mut dbg)? {
                DebugYield::Paused => {
                    if self.is_return_to_last_pause(&vm)? {
                        dbg.set_mode(self.last_motion.mode());
                        continue;
                    }
                    self.show_frame(&vm)?;
                    if let Flow::Quit = self.paused_loop(&mut vm, &mut dbg)? {
                        return Ok(());
                    }
                }
                DebugYield::Stop(Yield::Stop { command, args, .. }) => {
                    self.show_cues(&vm)?;
                    session::write_stop(self.loc, self.reg, self.out, false, command, &args)?;
                    let expect = expectation(self.reg, command, &args);
                    match self.answer_loop(&mut vm, &mut dbg, &expect)? {
                        Reply::Resume(v) => vm.resume(Input::Return(v))?,
                        Reply::Quit => return Ok(()),
                    }
                }
                DebugYield::Stop(_) => {
                    self.show_cues(&vm)?;
                    self.say("run-end", None)?;
                    return Ok(());
                }
                // `DebugYield` is non-exhaustive; nothing else is produced today.
                #[allow(unreachable_patterns)]
                _ => return Ok(()),
            }
        }
    }

    /// Whether this pause is on the line (and depth) the last shown pause was on: the line event after a call returns to its
    /// caller. It is skipped, unless a breakpoint is on the line. Also remembers this pause as the last one.
    fn is_return_to_last_pause(&mut self, vm: &Vm<'a>) -> Result<bool, Fatal> {
        let frames = self.frames(vm)?;
        let Some(f) = frames.first() else {
            return Ok(false);
        };
        let here = (f.module.clone(), f.line, f.name.clone(), frames.len());
        let same = self.last_pause.as_ref() == Some(&here)
            && !self
                .breakpoints
                .iter()
                .any(|b| b.module == f.module && b.line == f.line);
        if !same {
            self.last_pause = Some(here);
        }
        Ok(same)
    }

    fn set_texts(&mut self, vm: &mut Vm<'a>, path: &str) -> Result<(), Fatal> {
        let table = texts::read_table(self.loc, std::path::Path::new(path))
            .map_err(|l| Fatal::Lines(vec![l]))?;
        vm.set_texts(Some(table)).map_err(|issues| {
            Fatal::Lines(
                issues
                    .iter()
                    .map(|i| texts::issue_line(self.loc, path, i))
                    .collect(),
            )
        })
    }

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

    fn prompt(&mut self) -> Result<(), Fatal> {
        if self.interactive {
            write!(self.out, "{PROMPT}")?;
            self.out.flush()?;
        }
        Ok(())
    }

    /// Reads debugger commands at a pause until one runs the script on.
    fn paused_loop(&mut self, vm: &mut Vm<'a>, dbg: &mut Debugger) -> Result<Flow, Fatal> {
        loop {
            self.prompt()?;
            let mut line = String::new();
            if self.input.read_line(&mut line)? == 0 {
                return Ok(Flow::Quit);
            }
            let line = line.trim();
            let flow = if line.is_empty() {
                self.run_motion(dbg, self.last_motion);
                Flow::Go
            } else {
                self.command(vm, dbg, line)?
            };
            match flow {
                Flow::Stay => {}
                other => return Ok(other),
            }
        }
    }

    /// Reads the answer at a stop point. Lines starting with `:` are debugger commands.
    fn answer_loop(
        &mut self,
        vm: &mut Vm<'a>,
        dbg: &mut Debugger,
        expect: &Expect,
    ) -> Result<Reply, Fatal> {
        loop {
            self.prompt()?;
            let mut line = String::new();
            if self.input.read_line(&mut line)? == 0 {
                return match eof_answer(expect) {
                    Answer::Pick(i) => Ok(Reply::Resume(Value::Int(i as i64))),
                    Answer::Return(v) => Ok(Reply::Resume(v)),
                    Answer::EndOfInput => Err(Fatal::EventInputEnded),
                    _ => Ok(Reply::Resume(Value::Unit)),
                };
            }
            let raw = line.trim_end_matches(['\n', '\r']);
            match parse_answer(expect, raw) {
                Ok(Answer::Command) => {
                    let rest = raw.trim().strip_prefix(':').unwrap_or_default().trim();
                    if let Flow::Quit = self.command(vm, dbg, rest)? {
                        return Ok(Reply::Quit);
                    }
                }
                Ok(Answer::Advance) => return Ok(Reply::Resume(Value::Unit)),
                Ok(Answer::Pick(i)) => return Ok(Reply::Resume(Value::Int(i as i64))),
                Ok(Answer::Return(v)) => return Ok(Reply::Resume(v)),
                Ok(Answer::EndOfInput) => return Err(Fatal::EventInputEnded),
                Ok(Answer::Other) => self.complain("run-unknown-input", None)?,
                Err(e) => self.answer_error(expect, e)?,
            }
        }
    }

    /// Handles input that is not a valid answer: fatal for numbers and yes/no, a complaint (and waiting again) otherwise.
    fn answer_error(&mut self, expect: &Expect, e: AnswerError) -> Result<(), Fatal> {
        let count = match expect {
            Expect::Choice(ids) => ids.len(),
            _ => 0,
        };
        match e {
            AnswerError::BadInt(text) => Err(Fatal::BadInput("run-input-invalid-int", text)),
            AnswerError::BadConfirm(text) => {
                Err(Fatal::BadInput("run-input-invalid-confirm", text))
            }
            AnswerError::UnknownEvent(text) => {
                let mut args = one_arg("name", &text);
                args.set("names", event_names(expect));
                self.complain("run-event-unknown", Some(&args))
            }
            AnswerError::EmptyEvent => {
                let args = one_arg("names", &event_names(expect));
                self.complain("run-event-empty", Some(&args))
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
                self.complain(key, Some(&args))
            }
        }
    }

    fn run_motion(&mut self, dbg: &mut Debugger, motion: Motion) {
        self.last_motion = motion;
        dbg.set_mode(motion.mode());
    }

    /// Runs one debugger command line (without a leading `:`).
    fn command(&mut self, vm: &mut Vm<'a>, dbg: &mut Debugger, line: &str) -> Result<Flow, Fatal> {
        let (name, arg) = match line.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a.trim()),
            None => (line, ""),
        };
        let Some(cmd) = parse_command(name) else {
            let args = one_arg("command", name);
            self.complain("debug-unknown-command", Some(&args))?;
            return Ok(Flow::Stay);
        };
        // At a stop point the script cannot run on before the answer, so a motion only sets how it goes on afterwards.
        let at_pause = vm.is_paused();
        match cmd {
            Cmd::Motion(m) => {
                self.run_motion(dbg, m);
                Ok(if at_pause { Flow::Go } else { Flow::Stay })
            }
            Cmd::Quit => Ok(Flow::Quit),
            Cmd::Break => self.cmd_break(vm, dbg, arg).map(|()| Flow::Stay),
            Cmd::Delete => self.cmd_delete(dbg, arg).map(|()| Flow::Stay),
            Cmd::Breaks => self.cmd_breaks().map(|()| Flow::Stay),
            Cmd::Bt => self.cmd_bt(vm).map(|()| Flow::Stay),
            Cmd::Frame => self.cmd_frame(vm, arg).map(|()| Flow::Stay),
            Cmd::Up => {
                let next = self.frame + 1;
                self.select_frame(vm, next, "debug-frame-top")?;
                Ok(Flow::Stay)
            }
            Cmd::Down => {
                match self.frame.checked_sub(1) {
                    Some(prev) => self.select_frame(vm, prev, "debug-frame-bottom")?,
                    None => self.complain("debug-frame-bottom", None)?,
                }
                Ok(Flow::Stay)
            }
            Cmd::Vars => self.cmd_vars(vm, None).map(|()| Flow::Stay),
            Cmd::Print => {
                if arg.is_empty() {
                    self.complain("debug-print-usage", None)?;
                    Ok(Flow::Stay)
                } else {
                    self.cmd_vars(vm, Some(arg)).map(|()| Flow::Stay)
                }
            }
            Cmd::List => self.cmd_list(vm, arg).map(|()| Flow::Stay),
        }
    }

    fn frames(&self, vm: &Vm<'a>) -> Result<Vec<FrameInfo>, Fatal> {
        Ok(vm.call_stack()?)
    }

    fn source_line(&self, module: &str, line: u32) -> Option<&'a str> {
        let file = self.files.iter().find(|f| f.module == module)?;
        file.source.lines().nth(line.checked_sub(1)? as usize)
    }

    /// Writes the place of the selected frame and its source line.
    fn show_frame(&mut self, vm: &Vm<'a>) -> Result<(), Fatal> {
        let frames = self.frames(vm)?;
        let Some(f) = frames.get(self.frame) else {
            return Ok(());
        };
        writeln!(self.out, "-> {}:{} ({})", f.module, f.line, f.name)?;
        if let Some(text) = self.source_line(&f.module, f.line) {
            writeln!(self.out, "{} | {text}", f.line)?;
        }
        Ok(())
    }

    fn select_frame(&mut self, vm: &Vm<'a>, frame: usize, key: &str) -> Result<(), Fatal> {
        if frame >= self.frames(vm)?.len() {
            return self.complain(key, None);
        }
        self.frame = frame;
        self.show_frame(vm)
    }

    fn cmd_frame(&mut self, vm: &Vm<'a>, arg: &str) -> Result<(), Fatal> {
        let Ok(n) = arg.parse::<usize>() else {
            return self.complain_text("debug-bad-number", arg);
        };
        let count = self.frames(vm)?.len();
        if n >= count {
            let mut args = FluentArgs::new();
            args.set("frame", n as i64);
            args.set("max", count.saturating_sub(1) as i64);
            return self.complain("debug-frame-range", Some(&args));
        }
        self.frame = n;
        self.show_frame(vm)
    }

    fn complain_text(&mut self, key: &str, text: &str) -> Result<(), Fatal> {
        self.complain(key, Some(&one_arg("text", text)))
    }

    fn cmd_bt(&mut self, vm: &Vm<'a>) -> Result<(), Fatal> {
        for (i, f) in self.frames(vm)?.iter().enumerate() {
            let mark = if i == self.frame { '*' } else { ' ' };
            writeln!(self.out, "{mark}#{i} {}:{} ({})", f.module, f.line, f.name)?;
        }
        Ok(())
    }

    /// `vars` (everything visible from the selected frame) and `print <name>` (one variable).
    fn cmd_vars(&mut self, vm: &Vm<'a>, name: Option<&str>) -> Result<(), Fatal> {
        let locals = vm.frame_variables(self.frame)?;
        let others: Vec<Variable> = vm.variables().into_iter().filter(|v| !v.local).collect();
        let qualify = self.program.module_names().count() > 1;
        let Some(name) = name else {
            write_vars(self.out, &locals, false)?;
            write_vars(self.out, &others, qualify)?;
            return Ok(());
        };
        if let Some(v) = locals.iter().find(|v| v.name == name) {
            write_vars(self.out, std::slice::from_ref(v), false)?;
            return Ok(());
        }
        let found = others.iter().find(|v| {
            v.name == name
                || v.module
                    .as_deref()
                    .is_some_and(|m| format!("{m}.{}", v.name) == name)
        });
        match found {
            Some(v) => write_vars(self.out, std::slice::from_ref(v), qualify)?,
            None => self.complain_text("debug-var-unknown", name)?,
        }
        Ok(())
    }

    fn cmd_list(&mut self, vm: &Vm<'a>, arg: &str) -> Result<(), Fatal> {
        let context = if arg.is_empty() {
            LIST_CONTEXT
        } else {
            match arg.parse::<u32>() {
                Ok(n) => n,
                Err(_) => return self.complain_text("debug-bad-number", arg),
            }
        };
        let frames = self.frames(vm)?;
        let Some(f) = frames.get(self.frame) else {
            return Ok(());
        };
        let Some(file) = self.files.iter().find(|file| file.module == f.module) else {
            return self.complain_text("debug-no-source", &f.module);
        };
        let total = file.source.lines().count() as u32;
        let first = f.line.saturating_sub(context).max(1);
        let last = (f.line + context).min(total);
        let width = last.to_string().len();
        for (n, text) in (first..=last).zip(file.source.lines().skip(first as usize - 1)) {
            let mark = if n == f.line {
                "=>"
            } else if self
                .breakpoints
                .iter()
                .any(|b| b.module == f.module && b.line == n)
            {
                "* "
            } else {
                "  "
            };
            writeln!(self.out, "{mark} {n:>width$} | {text}")?;
        }
        Ok(())
    }

    fn sync_breakpoints(&self, dbg: &mut Debugger, module: &str) {
        let lines: Vec<u32> = self
            .breakpoints
            .iter()
            .filter(|b| b.module == module)
            .map(|b| b.line)
            .collect();
        dbg.set_breakpoints(module, &lines);
    }

    fn cmd_break(&mut self, vm: &Vm<'a>, dbg: &mut Debugger, arg: &str) -> Result<(), Fatal> {
        let (module, line_text) = match arg.rsplit_once(':') {
            Some((m, l)) => (m.trim().to_string(), l.trim()),
            None => {
                let frames = self.frames(vm)?;
                let module = frames.get(self.frame).map(|f| f.module.clone());
                (module.unwrap_or_default(), arg)
            }
        };
        let Ok(requested) = line_text.parse::<u32>() else {
            return self.complain("debug-break-usage", None);
        };
        if !self.program.module_names().any(|m| m == module) {
            return self.complain_text("debug-no-module", &module);
        }
        let lines = Debugger::breakable_lines(self.program, &module);
        let mut args = one_arg("module", &module);
        args.set("requested", requested as i64);
        let Some(&line) = lines.iter().find(|&&l| l >= requested) else {
            return self.complain("debug-break-none", Some(&args));
        };
        let number = self.next_number;
        self.next_number += 1;
        self.breakpoints.push(Breakpoint {
            number,
            module: module.clone(),
            line,
        });
        self.sync_breakpoints(dbg, &module);
        args.set("number", number as i64);
        args.set("line", line as i64);
        let key = if line == requested {
            "debug-break-set"
        } else {
            "debug-break-moved"
        };
        self.say(key, Some(&args))
    }

    fn cmd_delete(&mut self, dbg: &mut Debugger, arg: &str) -> Result<(), Fatal> {
        let removed: Vec<Breakpoint> = if arg.is_empty() {
            std::mem::take(&mut self.breakpoints)
        } else {
            let Ok(number) = arg.parse::<u32>() else {
                return self.complain_text("debug-bad-number", arg);
            };
            let Some(i) = self.breakpoints.iter().position(|b| b.number == number) else {
                return self.complain_text("debug-delete-unknown", arg);
            };
            vec![self.breakpoints.remove(i)]
        };
        let mut modules: Vec<&str> = removed.iter().map(|b| b.module.as_str()).collect();
        modules.dedup();
        for m in modules {
            self.sync_breakpoints(dbg, m);
        }
        Ok(())
    }

    fn cmd_breaks(&mut self) -> Result<(), Fatal> {
        if self.breakpoints.is_empty() {
            return self.say("debug-breaks-none", None);
        }
        for b in &self.breakpoints {
            writeln!(self.out, "{} {}:{}", b.number, b.module, b.line)?;
        }
        Ok(())
    }
}
