//! `yomi dap`: a Debug Adapter Protocol server over standard input and output (`spec/host/06-cli.md`, 6.19).
//!
//! The reading of messages is on its own thread, so that a running script can notice `pause` and `disconnect` at each line
//! event. Everything else happens on the calling thread.

use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use lingokit::{FluentArgs, Localizer};
use serde_json::{Value as Json, json};
use yomi_core::{
    DebugHook, DebugYield, Debugger, HookAction, Input, LineEvent, Program, Registry, StepMode,
    Value, Vm, VmConfig, VmError, Yield,
};
use yomi_std::audio::Audio;
use yomi_std::image::Image;
use yomi_std::screen::Screen;

use crate::diag::{error_line, vm_error_message};
use crate::modules::{self, SourceFile};
use crate::session::{
    self, Answer, AnswerError, Expect, expectation, one_arg, parse_answer, write_cues, write_stop,
    write_vars,
};
use crate::texts;

const THREAD_ID: i64 = 1;

/// A message read from standard input, or why it could not be read.
type Incoming = Result<Json, String>;

/// Reads one `Content-Length` framed message. `Ok(None)` at the end of the input.
fn read_message<R: BufRead>(r: &mut R) -> Result<Option<Json>, String> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if r.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| format!("bad Content-Length: {value}"))?,
            );
        }
    }
    let length = length.ok_or("missing Content-Length")?;
    let mut body = vec![0; length];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn spawn_reader<R: Read + Send + 'static>(input: R) -> Receiver<Incoming> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = io::BufReader::new(input);
        loop {
            match read_message(&mut reader) {
                Ok(Some(m)) => {
                    if tx.send(Ok(m)).is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
    });
    rx
}

/// Writes framed messages and numbers them.
struct Io<O> {
    out: O,
    seq: i64,
}

impl<O: Write> Io<O> {
    fn send(&mut self, mut message: Json) -> io::Result<()> {
        self.seq += 1;
        message["seq"] = json!(self.seq);
        let body = serde_json::to_vec(&message)?;
        write!(self.out, "Content-Length: {}\r\n\r\n", body.len())?;
        self.out.write_all(&body)?;
        self.out.flush()
    }

    fn respond(&mut self, request: &Json, result: Result<Json, String>) -> io::Result<()> {
        let mut m = json!({
            "type": "response",
            "request_seq": request["seq"],
            "command": request["command"],
        });
        match result {
            Ok(body) => {
                m["success"] = json!(true);
                m["body"] = body;
            }
            Err(message) => {
                m["success"] = json!(false);
                m["message"] = json!(message);
            }
        }
        self.send(m)
    }

    fn event(&mut self, name: &str, body: Json) -> io::Result<()> {
        self.send(json!({ "type": "event", "event": name, "body": body }))
    }

    fn output(&mut self, category: &str, text: &str) -> io::Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.event("output", json!({ "category": category, "output": text }))
    }
}

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

    fn parse(name: &str) -> Option<Motion> {
        match name {
            "step" | "s" => Some(Motion::Step),
            "next" | "n" => Some(Motion::Next),
            "finish" | "fin" => Some(Motion::Finish),
            "continue" | "c" => Some(Motion::Continue),
            _ => None,
        }
    }
}

/// Where the script is while the adapter waits for requests.
#[allow(clippy::enum_variant_names)]
enum Stop {
    /// Not started, or finished.
    Idle,
    /// Paused before a line.
    Paused,
    /// At a stop point of the script, waiting for an answer.
    AtStop(Expect),
}

/// What the hook found while the script ran.
enum Quit {
    Request(Json),
    Eof,
}

/// The debugger as the hook of a run: also looks at the incoming messages at each line event.
struct Hook<'h, O> {
    dbg: &'h mut Debugger,
    rx: &'h Receiver<Incoming>,
    io: &'h mut Io<O>,
    loc: &'h Localizer,
    breaks: &'h HashMap<String, Vec<u32>>,
    pause_requested: bool,
    at_breakpoint: bool,
    quit: Option<Quit>,
    failure: Option<io::Error>,
}

impl<O: Write> DebugHook for Hook<'_, O> {
    fn on_line(&mut self, event: &LineEvent<'_>) -> HookAction {
        if self.dbg.on_line(event) == HookAction::Pause {
            self.at_breakpoint = self
                .breaks
                .get(event.module)
                .is_some_and(|lines| lines.contains(&event.line));
            return HookAction::Pause;
        }
        loop {
            match self.rx.try_recv() {
                Ok(Ok(request)) => match request["command"].as_str().unwrap_or_default() {
                    "pause" => {
                        if let Err(e) = self.io.respond(&request, Ok(json!({}))) {
                            self.failure = Some(e);
                        }
                        self.pause_requested = true;
                        return HookAction::Pause;
                    }
                    "disconnect" | "terminate" => {
                        self.quit = Some(Quit::Request(request));
                        return HookAction::Pause;
                    }
                    _ => {
                        let message = self.loc.fmt("dap-running", None);
                        if let Err(e) = self.io.respond(&request, Err(message)) {
                            self.failure = Some(e);
                            return HookAction::Pause;
                        }
                    }
                },
                Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                    self.quit = Some(Quit::Eof);
                    return HookAction::Pause;
                }
                Err(TryRecvError::Empty) => return HookAction::Continue,
            }
        }
    }
}

/// What `go` and the handlers tell the serving loop.
enum Flow {
    Stay,
    Quit,
}

/// The script as launched.
struct Launched {
    files: Vec<SourceFile>,
    program: &'static Program,
    reg: &'static Registry,
    vm: Vm<'static>,
    start_module: String,
}

struct Server<'a, O> {
    loc: &'a Localizer,
    io: Io<O>,
    rx: Receiver<Incoming>,
    launched: Option<Launched>,
    stop_on_entry: bool,
    /// The lines of the breakpoints that were placed, by module.
    breaks: HashMap<String, Vec<u32>>,
    dbg: Debugger,
    last_motion: Motion,
    /// Where the last reported pause was (module, line, name, depth).
    last_pause: Option<(String, u32, String, usize)>,
    stop: Stop,
    entry_pending: bool,
}

/// Runs the server until the client disconnects or the input ends. Returns the exit code.
pub fn run<R: Read + Send + 'static, O: Write>(loc: &Localizer, input: R, out: O) -> i32 {
    let mut server = Server {
        loc,
        io: Io { out, seq: 0 },
        rx: spawn_reader(input),
        launched: None,
        stop_on_entry: true,
        breaks: HashMap::new(),
        dbg: Debugger::new(),
        last_motion: Motion::Step,
        last_pause: None,
        stop: Stop::Idle,
        entry_pending: false,
    };
    match server.serve() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn arg_str<'j>(args: &'j Json, name: &str) -> Option<&'j str> {
    args.get(name).and_then(Json::as_str)
}

fn shown_path(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(|_| path.to_string(), |p| p.display().to_string())
}

impl<'a, O: Write> Server<'a, O> {
    fn serve(&mut self) -> io::Result<i32> {
        loop {
            let request = match self.rx.recv() {
                Ok(Ok(request)) => request,
                Ok(Err(e)) => {
                    eprintln!("{e}");
                    return Ok(1);
                }
                Err(_) => return Ok(0),
            };
            if let Flow::Quit = self.handle(&request)? {
                return Ok(0);
            }
        }
    }

    fn msg(&self, key: &str) -> String {
        self.loc.fmt(key, None)
    }

    fn handle(&mut self, request: &Json) -> io::Result<Flow> {
        let args = request.get("arguments").cloned().unwrap_or(json!({}));
        let command = request["command"].as_str().unwrap_or_default();
        match command {
            "initialize" => {
                self.io.respond(
                    request,
                    Ok(json!({ "supportsConfigurationDoneRequest": true })),
                )?;
                self.io.event("initialized", json!({}))?;
            }
            "launch" => self.launch(request, &args)?,
            "setBreakpoints" => {
                let r = self.set_breakpoints(&args);
                self.io.respond(request, r)?;
            }
            "setExceptionBreakpoints" => self.io.respond(request, Ok(json!({})))?,
            "configurationDone" => {
                self.io.respond(request, Ok(json!({})))?;
                if self.launched.is_some() {
                    self.dbg.set_mode(if self.stop_on_entry {
                        StepMode::In
                    } else {
                        StepMode::Continue
                    });
                    self.entry_pending = self.stop_on_entry;
                    return self.go();
                }
            }
            "threads" => self.io.respond(
                request,
                Ok(json!({ "threads": [{ "id": THREAD_ID, "name": "main" }] })),
            )?,
            "stackTrace" => {
                let r = self.stack_trace();
                self.io.respond(request, r)?;
            }
            "scopes" => {
                let frame = args["frameId"].as_i64().unwrap_or(0);
                self.io.respond(
                    request,
                    Ok(json!({ "scopes": [
                        { "name": "Locals", "variablesReference": frame * 2 + 1, "expensive": false },
                        { "name": "Globals", "variablesReference": frame * 2 + 2, "expensive": false },
                    ] })),
                )?;
            }
            "variables" => {
                let r = self.variables(args["variablesReference"].as_i64().unwrap_or(0));
                self.io.respond(request, r)?;
            }
            "continue" | "next" | "stepIn" | "stepOut" => {
                let motion = match command {
                    "continue" => Motion::Continue,
                    "next" => Motion::Next,
                    "stepIn" => Motion::Step,
                    _ => Motion::Finish,
                };
                return self.motion(request, motion);
            }
            "pause" => self.io.respond(request, Ok(json!({})))?,
            "evaluate" => return self.evaluate(request, &args),
            "disconnect" | "terminate" => {
                self.io.respond(request, Ok(json!({})))?;
                return Ok(Flow::Quit);
            }
            other => {
                let mut a = FluentArgs::new();
                a.set("command", other.to_string());
                let message = self.loc.fmt("dap-unknown-request", Some(&a));
                self.io.respond(request, Err(message))?;
            }
        }
        Ok(Flow::Stay)
    }

    fn launch(&mut self, request: &Json, args: &Json) -> io::Result<()> {
        match self.prepare(args) {
            Ok(launched) => {
                self.stop_on_entry = args["stopOnEntry"].as_bool().unwrap_or(true);
                self.launched = Some(launched);
                self.io.respond(request, Ok(json!({})))
            }
            Err(message) => {
                self.io.respond(request, Err(message))?;
                self.io.event("terminated", json!({}))
            }
        }
    }

    /// Collects, compiles and starts the script. Diagnostics go to standard error.
    fn prepare(&mut self, args: &Json) -> Result<Launched, String> {
        let Some(program_path) = arg_str(args, "program") else {
            return Err(self.msg("dap-launch-needs-program"));
        };
        let root = arg_str(args, "root").map(Path::new);
        let (files, entry) = match modules::collect(Path::new(program_path), root) {
            Ok(found) => found,
            Err(errors) => {
                for e in &errors {
                    eprintln!("{}", texts::collect_error_line(self.loc, program_path, e));
                }
                return Err(self.msg("dap-launch-failed"));
            }
        };
        let start = modules::start_label(&files, entry, arg_str(args, "start").map(str::to_string));
        let reg: &'static Registry = Box::leak(Box::new(session::registry()));
        let Some(program) =
            session::compile_files(self.loc, reg, &files, &start, &mut io::stderr().lock())
        else {
            return Err(self.msg("dap-launch-failed"));
        };
        let program: &'static Program = Box::leak(Box::new(program));
        let seed = args["seed"].as_u64().unwrap_or_else(session::time_seed);
        let mut vm = Vm::new(program, reg, VmConfig::default().start(&start).seed(seed))
            .map_err(|e| self.describe(&files, &start, &e))?;
        if let Some(path) = arg_str(args, "texts") {
            let table = texts::read_table(self.loc, Path::new(path))?;
            vm.set_texts(Some(table)).map_err(|issues| {
                issues
                    .iter()
                    .map(|i| texts::issue_line(self.loc, path, i))
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
        }
        let start_module = start.split_once('.').map_or("", |(m, _)| m).to_string();
        Ok(Launched {
            files,
            program,
            reg,
            vm,
            start_module,
        })
    }

    fn describe(&self, files: &[SourceFile], start: &str, e: &VmError) -> String {
        let module = start.split_once('.').map_or("", |(m, _)| m);
        let file = modules::path_of(files, e.module.as_deref().unwrap_or(module));
        error_line(self.loc, &file, &vm_error_message(self.loc, e))
    }

    fn set_breakpoints(&mut self, args: &Json) -> Result<Json, String> {
        let path = arg_str(&args["source"], "path").map(shown_path);
        let requested: Vec<Json> = args["breakpoints"].as_array().cloned().unwrap_or_default();
        let module = path.as_deref().and_then(|p| {
            let l = self.launched.as_ref()?;
            l.files
                .iter()
                .find(|f| shown_path(&f.path) == p && !f.module.is_empty())
                .map(|f| f.module.clone())
        });
        let breakable = match (&module, &self.launched) {
            (Some(m), Some(l)) => Debugger::breakable_lines(l.program, m),
            _ => Vec::new(),
        };
        let mut placed: Vec<u32> = Vec::new();
        let mut reported = Vec::new();
        for bp in &requested {
            let line = bp["line"].as_u64().unwrap_or(0) as u32;
            if bp.get("condition").is_some()
                || bp.get("hitCondition").is_some()
                || bp.get("logMessage").is_some()
            {
                reported.push(json!({ "verified": false, "line": line, "message": self.msg("dap-unsupported-condition") }));
            } else if module.is_none() {
                reported.push(json!({ "verified": false, "line": line, "message": self.msg("dap-not-a-script") }));
            } else if let Some(&at) = breakable.iter().find(|&&l| l >= line) {
                placed.push(at);
                reported.push(json!({ "verified": true, "line": at }));
            } else {
                reported.push(
                    json!({ "verified": false, "line": line, "message": self.msg("dap-no-line") }),
                );
            }
        }
        if let Some(m) = module {
            self.dbg.set_breakpoints(&m, &placed);
            if placed.is_empty() {
                self.breaks.remove(&m);
            } else {
                self.breaks.insert(m, placed);
            }
        }
        Ok(json!({ "breakpoints": reported }))
    }

    fn stop_message(&self) -> String {
        self.msg("dap-not-stopped")
    }

    fn stack_trace(&mut self) -> Result<Json, String> {
        let not_stopped = self.stop_message();
        let Some(l) = &self.launched else {
            return Err(not_stopped);
        };
        let frames = l.vm.call_stack().map_err(|_| not_stopped)?;
        let list: Vec<Json> = frames
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let path = shown_path(&modules::path_of(&l.files, &f.module));
                let name = Path::new(&path)
                    .file_name()
                    .map_or(path.clone(), |n| n.to_string_lossy().into_owned());
                json!({
                    "id": i,
                    "name": f.name,
                    "source": { "name": name, "path": path },
                    "line": f.line,
                    "column": 1,
                })
            })
            .collect();
        Ok(json!({ "stackFrames": list, "totalFrames": list.len() }))
    }

    fn variables(&mut self, reference: i64) -> Result<Json, String> {
        let not_stopped = self.stop_message();
        let Some(l) = &self.launched else {
            return Err(not_stopped);
        };
        let frame = ((reference - 1) / 2) as usize;
        let vars = if reference % 2 == 1 {
            l.vm.frame_variables(frame).map_err(|_| not_stopped)?
        } else {
            l.vm.variables().into_iter().filter(|v| !v.local).collect()
        };
        let qualify = l.program.module_names().count() > 1;
        let list: Vec<Json> = vars
            .iter()
            .map(|v| {
                let name = match &v.module {
                    Some(m) if qualify => format!("{m}.{}", v.name),
                    _ => v.name.clone(),
                };
                json!({ "name": name, "value": v.value.to_repl_text(), "variablesReference": 0 })
            })
            .collect();
        Ok(json!({ "variables": list }))
    }

    fn motion(&mut self, request: &Json, motion: Motion) -> io::Result<Flow> {
        if let Stop::AtStop(_) = self.stop {
            let message = self.msg("dap-answer-first");
            self.io.respond(request, Err(message))?;
            return Ok(Flow::Stay);
        }
        if !matches!(self.stop, Stop::Paused) {
            let message = self.stop_message();
            self.io.respond(request, Err(message))?;
            return Ok(Flow::Stay);
        }
        self.last_motion = motion;
        self.dbg.set_mode(motion.mode());
        let body = if request["command"] == "continue" {
            json!({ "allThreadsContinued": true })
        } else {
            json!({})
        };
        self.io.respond(request, Ok(body))?;
        self.go()
    }

    fn evaluate(&mut self, request: &Json, args: &Json) -> io::Result<Flow> {
        let expression = args["expression"].as_str().unwrap_or_default();
        let frame = args["frameId"].as_u64().unwrap_or(0) as usize;
        if args["context"] != "repl" {
            let r = self.lookup(expression.trim(), frame);
            self.io.respond(
                request,
                r.map(|value| json!({ "result": value, "variablesReference": 0 })),
            )?;
            return Ok(Flow::Stay);
        }
        let reply = |result: &str| json!({ "result": result, "variablesReference": 0 });
        match std::mem::replace(&mut self.stop, Stop::Idle) {
            Stop::Idle => {
                let message = self.stop_message();
                self.io.respond(request, Err(message))?;
                Ok(Flow::Stay)
            }
            Stop::Paused => {
                self.stop = Stop::Paused;
                self.repl_command(request, expression.trim(), frame, &reply)
            }
            Stop::AtStop(expect) => {
                let raw = expression.trim_end_matches(['\n', '\r']);
                let resume = match parse_answer(&expect, raw) {
                    Ok(Answer::Command) => {
                        self.stop = Stop::AtStop(expect);
                        let rest = raw.trim().strip_prefix(':').unwrap_or_default().trim();
                        return self.repl_command(request, rest, frame, &reply);
                    }
                    Ok(Answer::Advance) => Ok(Value::Unit),
                    Ok(Answer::Pick(i)) => Ok(Value::Int(i as i64)),
                    Ok(Answer::Return(v)) => Ok(v),
                    Ok(Answer::EndOfInput) => Err("run-event-input-ended".to_string()),
                    Ok(Answer::Other) => Err("run-unknown-input".to_string()),
                    Err(e) => Err(answer_error_message(self.loc, &expect, e)),
                };
                match resume {
                    Ok(value) => {
                        self.io.respond(request, Ok(reply("")))?;
                        let Some(l) = &mut self.launched else {
                            return Ok(Flow::Stay);
                        };
                        match l.vm.resume(Input::Return(value)) {
                            Ok(()) => self.go(),
                            Err(e) => self.fail(&e),
                        }
                    }
                    Err(message) => {
                        self.stop = Stop::AtStop(expect);
                        let message = if message.starts_with("run-") {
                            self.loc.fmt(&message, None)
                        } else {
                            message
                        };
                        self.io.respond(request, Err(message))?;
                        Ok(Flow::Stay)
                    }
                }
            }
        }
    }

    /// A debugger command typed in the Debug Console.
    fn repl_command(
        &mut self,
        request: &Json,
        line: &str,
        frame: usize,
        reply: &dyn Fn(&str) -> Json,
    ) -> io::Result<Flow> {
        let (name, arg) = match line.split_once(char::is_whitespace) {
            Some((n, a)) => (n, a.trim()),
            None => (line, ""),
        };
        let at_stop = matches!(self.stop, Stop::AtStop(_));
        if let Some(motion) = Motion::parse(name) {
            self.last_motion = motion;
            self.dbg.set_mode(motion.mode());
            self.io.respond(request, Ok(reply("")))?;
            return if at_stop { Ok(Flow::Stay) } else { self.go() };
        }
        let result = self.text_command(name, arg, frame);
        self.io.respond(request, result.map(|t| reply(&t)))?;
        Ok(Flow::Stay)
    }

    fn text_command(&mut self, name: &str, arg: &str, frame: usize) -> Result<String, String> {
        let not_stopped = self.stop_message();
        let Some(l) = &self.launched else {
            return Err(not_stopped);
        };
        let mut buf: Vec<u8> = Vec::new();
        match name {
            "bt" | "where" => {
                let frames = l.vm.call_stack().map_err(|_| not_stopped)?;
                for (i, f) in frames.iter().enumerate() {
                    let mark = if i == frame { '*' } else { ' ' };
                    let _ = writeln!(buf, "{mark}#{i} {}:{} ({})", f.module, f.line, f.name);
                }
            }
            "vars" | "v" | "print" | "p" => {
                let locals = l.vm.frame_variables(frame).map_err(|_| not_stopped)?;
                let others: Vec<_> = l.vm.variables().into_iter().filter(|v| !v.local).collect();
                let qualify = l.program.module_names().count() > 1;
                if name.starts_with('p') {
                    let one = locals
                        .iter()
                        .find(|v| v.name == arg)
                        .map(|v| (v, false))
                        .or_else(|| others.iter().find(|v| v.name == arg).map(|v| (v, qualify)));
                    let Some((v, q)) = one else {
                        return Err(self.lookup_failure(arg));
                    };
                    let _ = write_vars(&mut buf, std::slice::from_ref(v), q);
                } else {
                    let _ = write_vars(&mut buf, &locals, false);
                    let _ = write_vars(&mut buf, &others, qualify);
                }
            }
            "list" | "l" => {
                let frames = l.vm.call_stack().map_err(|_| not_stopped)?;
                let f = frames.get(frame).ok_or_else(|| self.stop_message())?;
                let file = l.files.iter().find(|x| x.module == f.module);
                let source = file.map(|x| x.source.as_str()).unwrap_or_default();
                let first = f.line.saturating_sub(5).max(1);
                for (n, text) in (first..=f.line + 5).zip(source.lines().skip(first as usize - 1)) {
                    let mark = if n == f.line { "=>" } else { "  " };
                    let _ = writeln!(buf, "{mark} {n} | {text}");
                }
            }
            other => {
                let mut a = FluentArgs::new();
                a.set("command", other.to_string());
                return Err(self.loc.fmt("debug-unknown-command", Some(&a)));
            }
        }
        Ok(String::from_utf8_lossy(&buf).trim_end().to_string())
    }

    fn lookup_failure(&self, name: &str) -> String {
        self.loc
            .fmt("debug-var-unknown", Some(&one_arg("text", name)))
    }

    /// The value of a variable by name (watch and hover).
    fn lookup(&self, name: &str, frame: usize) -> Result<String, String> {
        let not_stopped = self.stop_message();
        let Some(l) = &self.launched else {
            return Err(not_stopped);
        };
        let locals = l.vm.frame_variables(frame).map_err(|_| not_stopped)?;
        let found = locals.into_iter().find(|v| v.name == name).or_else(|| {
            l.vm.variables().into_iter().find(|v| {
                !v.local
                    && (v.name == name
                        || v.module
                            .as_deref()
                            .is_some_and(|m| format!("{m}.{}", v.name) == name))
            })
        });
        found
            .map(|v| v.value.to_repl_text())
            .ok_or_else(|| self.lookup_failure(name))
    }

    /// Reports a runtime error and ends the session of the script.
    fn fail(&mut self, e: &VmError) -> io::Result<Flow> {
        let Some(l) = &self.launched else {
            return Ok(Flow::Stay);
        };
        let file = modules::path_of(
            &l.files,
            e.module.as_deref().unwrap_or(l.start_module.as_str()),
        );
        let text = format!(
            "{}\n",
            error_line(self.loc, &file, &vm_error_message(self.loc, e))
        );
        self.stop = Stop::Idle;
        self.io.output("stderr", &text)?;
        self.io.event("terminated", json!({}))?;
        self.io.event("exited", json!({ "exitCode": 1 }))?;
        Ok(Flow::Stay)
    }

    /// Runs the script until it pauses, reaches a stop point or ends, and reports it to the client.
    fn go(&mut self) -> io::Result<Flow> {
        loop {
            let Some(l) = self.launched.as_mut() else {
                return Ok(Flow::Stay);
            };
            self.stop = Stop::Idle;
            let mut hook = Hook {
                dbg: &mut self.dbg,
                rx: &self.rx,
                io: &mut self.io,
                loc: self.loc,
                breaks: &self.breaks,
                pause_requested: false,
                at_breakpoint: false,
                quit: None,
                failure: None,
            };
            let outcome = l.vm.run_debug(&mut hook);
            let (pause_requested, at_breakpoint, quit, failure) = (
                hook.pause_requested,
                hook.at_breakpoint,
                hook.quit.take(),
                hook.failure.take(),
            );
            if let Some(e) = failure {
                return Err(e);
            }
            if let Some(quit) = quit {
                if let Quit::Request(request) = quit {
                    self.io.respond(&request, Ok(json!({})))?;
                }
                return Ok(Flow::Quit);
            }
            match outcome {
                Err(e) => return self.fail(&e),
                Ok(DebugYield::Paused) => {
                    let frames = l.vm.call_stack().unwrap_or_default();
                    if let Some(f) = frames.first() {
                        let here = (f.module.clone(), f.line, f.name.clone(), frames.len());
                        let same = !pause_requested
                            && !at_breakpoint
                            && self.last_motion != Motion::Continue
                            && self.last_pause.as_ref() == Some(&here)
                            && !self.entry_pending;
                        if same {
                            self.dbg.set_mode(self.last_motion.mode());
                            continue;
                        }
                        self.last_pause = Some(here);
                    }
                    let reason = if self.entry_pending {
                        "entry"
                    } else if pause_requested {
                        "pause"
                    } else if at_breakpoint {
                        "breakpoint"
                    } else {
                        "step"
                    };
                    self.entry_pending = false;
                    self.stop = Stop::Paused;
                    self.io.event(
                        "stopped",
                        json!({ "reason": reason, "threadId": THREAD_ID, "allThreadsStopped": true }),
                    )?;
                    return Ok(Flow::Stay);
                }
                Ok(DebugYield::Stop(Yield::Stop { command, args, .. })) => {
                    let mut buf: Vec<u8> = Vec::new();
                    cues(self.loc, &l.vm, &mut buf);
                    let _ = write_stop(self.loc, l.reg, &mut buf, false, command, &args);
                    let expect = expectation(l.reg, command, &args);
                    self.entry_pending = false;
                    self.stop = Stop::AtStop(expect);
                    self.io.output("stdout", &String::from_utf8_lossy(&buf))?;
                    self.io.event(
                        "stopped",
                        json!({
                            "reason": "step",
                            "description": self.loc.fmt("dap-stop-point", None),
                            "threadId": THREAD_ID,
                            "allThreadsStopped": true,
                        }),
                    )?;
                    return Ok(Flow::Stay);
                }
                Ok(DebugYield::Stop(_)) => {
                    let mut buf: Vec<u8> = Vec::new();
                    cues(self.loc, &l.vm, &mut buf);
                    let _ = writeln!(buf, "{}", self.loc.fmt("run-end", None));
                    self.stop = Stop::Idle;
                    self.io.output("stdout", &String::from_utf8_lossy(&buf))?;
                    self.io.event("terminated", json!({}))?;
                    self.io.event("exited", json!({ "exitCode": 0 }))?;
                    return Ok(Flow::Stay);
                }
                #[allow(unreachable_patterns)]
                Ok(_) => return Ok(Flow::Stay),
            }
        }
    }
}

fn cues(loc: &Localizer, vm: &Vm<'static>, buf: &mut Vec<u8>) {
    let _ = write_cues(
        loc,
        buf,
        vm.package_state::<Audio>(),
        vm.package_state::<Image>(),
        vm.package_state::<Screen>(),
    );
}

/// The text for an answer that does not fit the stop point.
fn answer_error_message(loc: &Localizer, expect: &Expect, e: AnswerError) -> String {
    let count = match expect {
        Expect::Choice(ids) => ids.len() as i64,
        _ => 0,
    };
    match e {
        AnswerError::BadInt(text) => {
            loc.fmt("run-input-invalid-int", Some(&one_arg("text", &text)))
        }
        AnswerError::BadConfirm(text) => {
            loc.fmt("run-input-invalid-confirm", Some(&one_arg("text", &text)))
        }
        AnswerError::UnknownEvent(text) => {
            let mut a = one_arg("name", &text);
            a.set("names", session::event_names(expect));
            loc.fmt("run-event-unknown", Some(&a))
        }
        AnswerError::EmptyEvent => loc.fmt(
            "run-event-empty",
            Some(&one_arg("names", &session::event_names(expect))),
        ),
        AnswerError::UnknownId(text) | AnswerError::Invalid(text) => {
            let key = if text.starts_with('#') {
                "run-choose-unknown-id"
            } else {
                "run-choose-invalid"
            };
            let mut a = FluentArgs::new();
            a.set("count", count);
            a.set("id", text.trim_start_matches('#').to_string());
            loc.fmt(key, Some(&a))
        }
    }
}
