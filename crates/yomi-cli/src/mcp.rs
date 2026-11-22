//! `yomi mcp` (an MCP server that talks over standard input and output).
//!
//! The tools are `check`, `format`, `reserved_words`, `diagnostic_codes`, `spec`, `run` and `test`. Descriptions are written in English and
//! can be translated with `mcp-*` keys (only when `YOMI_MCP_LOCALIZE_SCHEMA=true`). The text of results is always in the
//! display language.

mod sandbox;
mod spec;

use lingokit::Localizer;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ErrorData, ServerHandler, ServiceExt, schemars, tool, tool_handler, tool_router};
use serde::{Deserialize, Serialize};
use yomi_core::{
    CONTEXTUAL_KEYWORDS, CompileOptions, ContextualPosition, KEYWORDS, Severity, all_codes,
    compile_modules_with_options,
};

use crate::codes;
use crate::diag::diagnostic_message;
use crate::modules;

/// One module of a script.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ModuleSource {
    /// Module name (`story::ch1`; ASCII letters, digits and `_`, separated by `::`)
    pub name: String,
    /// The source text of the module
    pub source: String,
}

/// The arguments of `check`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckParams {
    /// Source text of the entry module (named `main`); give this or `path`
    pub source: Option<String>,
    /// Other modules that the entry module imports (with `source`)
    pub modules: Option<Vec<ModuleSource>>,
    /// Path of the entry script file on this machine; give this or `source`
    pub path: Option<String>,
    /// Directory to search for modules (with `path`); defaults to the directory of the script
    pub root: Option<String>,
    /// Path of a declaration file (the JSON written by `yomi decls`) on this machine; defaults to the built-in registry
    pub decls: Option<String>,
}

/// The arguments of `format`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FormatParams {
    /// Source text of the entry module (named `main`); give this or `path`
    pub source: Option<String>,
    /// Other modules that the entry module imports (with `source`)
    pub modules: Option<Vec<ModuleSource>>,
    /// Path of the entry script file on this machine; give this or `source`
    pub path: Option<String>,
    /// Directory to search for modules (with `path`); defaults to the directory of the script
    pub root: Option<String>,
}

/// One diagnostic.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DiagnosticItem {
    /// Module name
    pub module: String,
    /// File path when the script was read from a file, otherwise the module name
    pub file: String,
    /// Line (1-based)
    pub line: u32,
    /// Column (1-based, counted in characters)
    pub column: u32,
    /// `error` or `warning`
    pub severity: String,
    /// Diagnostic code (see `diagnostic_codes`)
    pub code: String,
    /// Message in the display language
    pub message: String,
}

/// The result of `check`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CheckResult {
    /// True when there are no errors (warnings are allowed)
    pub ok: bool,
    /// Number of errors
    pub errors: u32,
    /// Number of warnings
    pub warnings: u32,
    /// The diagnostics, in the order the compiler reports them
    pub diagnostics: Vec<DiagnosticItem>,
}

/// The result of `format`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FormatResult {
    /// True when the script could be formatted
    pub ok: bool,
    /// The formatted script (absent when `ok` is false)
    pub formatted: Option<String>,
    /// True when the formatted script differs from the original
    pub changed: bool,
    /// The syntax errors (when `ok` is false)
    pub diagnostics: Vec<DiagnosticItem>,
}

/// One reserved word.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct KeywordItem {
    /// The word
    pub word: String,
    /// `declaration`, `statement`, `modifier`, `type` or `value`
    pub kind: String,
    /// True when the syntax that starts with this word opens a block with `{` at the end of the line
    pub opens_block: bool,
}

/// One contextual word.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ContextualItem {
    /// The word (not reserved; it has a meaning only in this position)
    pub word: String,
    /// `choice_option`, `import_alias` or `import_namespace`
    pub position: String,
}

/// The result of `reserved_words`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReservedWords {
    /// The reserved words (they cannot be names, except after `.` in a member name)
    pub keywords: Vec<KeywordItem>,
    /// The words that have a meaning only in a fixed position
    pub contextual: Vec<ContextualItem>,
}

/// The arguments of `diagnostic_codes`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CodesParams {
    /// Also list the error codes of the standard packages and the core commands (default false)
    pub packages: Option<bool>,
}

/// One code.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CodeItem {
    /// `language`, `package` or `core`
    pub kind: String,
    /// The group of a language code (`parse`, `compile`, ...); empty for the others
    pub group: String,
    /// The code
    pub code: String,
    /// `error` or `warning`
    pub severity: String,
    /// The names of the arguments of the message
    pub args: Vec<String>,
    /// The message in the display language, with `<name>` in place of the arguments
    pub message: String,
}

/// The result of `diagnostic_codes`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CodesResult {
    /// The codes
    pub codes: Vec<CodeItem>,
}

/// The arguments of `spec`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SpecParams {
    /// Document id such as `language/01-lexical`; omit to list the documents
    pub part: Option<String>,
}

/// The heading of one spec document.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SpecPart {
    /// Document id (the path under `spec/` without `.md`)
    pub id: String,
    /// The title
    pub title: String,
}

/// The result of `spec`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SpecResult {
    /// The documents that can be read
    pub parts: Vec<SpecPart>,
    /// The id of the document in `text`
    pub id: Option<String>,
    /// The Markdown text of the requested document (in Japanese)
    pub text: Option<String>,
}

/// The arguments of `run`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RunParams {
    /// Source text of the entry module (named `main`)
    pub source: String,
    /// Other modules that the entry module imports
    pub modules: Option<Vec<ModuleSource>>,
    /// Lines for standard input: answers to choices and input.* stops (one per line)
    pub input: Option<String>,
    /// Seed for random (the run is deterministic with a seed)
    pub seed: Option<u64>,
    /// Label to start from (a label name or module.label; defaults to `start` of `main`)
    pub start: Option<String>,
}

/// The result of `run`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RunResult {
    /// Standard output of the script (the text lines and stop points)
    pub stdout: String,
    /// Standard error (diagnostics and runtime errors, in the display language of the server)
    pub stderr: String,
    /// Exit code; null when the run was killed
    pub exit_code: Option<i32>,
    /// True when the time limit was exceeded and the run was killed
    pub timed_out: bool,
    /// True when the output exceeded the limit and the rest was dropped
    pub truncated: bool,
}

/// The arguments of `test`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TestParams {
    /// Source text of the entry module (named `main`)
    pub source: String,
    /// Other modules that the entry module imports
    pub modules: Option<Vec<ModuleSource>>,
    /// Label to start from (a label name or module.label; defaults to `start` of `main`)
    pub start: Option<String>,
    /// Number of random seeds to explore, `0` to `seeds - 1` (default 1; at least 1)
    pub seeds: Option<u64>,
    /// Upper bound of the number of states to visit (default 10000, at most 100000)
    pub max_states: Option<usize>,
    /// Upper bound of the number of instructions of one run (defaults to the VM default)
    pub max_steps: Option<u64>,
    /// Data of events to try at `event.wait`: event name to a list of data (each is tried); a name without data is
    /// tried with empty data
    pub events: Option<std::collections::BTreeMap<String, Vec<String>>>,
}

/// The result of `test`.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TestResult {
    /// The report of the exploration (states, paths, errors, unanswered stops, unreached stops, unchosen options and
    /// unreached labels); null when no report could be made (compile errors, killed, output cut)
    pub report: Option<serde_json::Value>,
    /// Standard error (diagnostics, in the display language of the server)
    pub stderr: String,
    /// Exit code (1 also when the report has error paths or unanswered stops); null when the run was killed
    pub exit_code: Option<i32>,
    /// True when the time limit was exceeded and the run was killed
    pub timed_out: bool,
    /// True when the output exceeded the limit and the rest was dropped
    pub truncated: bool,
}

/// The time limit of `test`.
const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// The default and the upper bound of `max_states` of `test`.
const TEST_MAX_STATES_DEFAULT: usize = 10_000;
const TEST_MAX_STATES_LIMIT: usize = 100_000;

/// The MCP server.
pub struct Server {
    loc: Localizer,
    tool_router: ToolRouter<Self>,
}

fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        _ => "warning",
    }
}

fn position_name(p: ContextualPosition) -> &'static str {
    match p {
        ContextualPosition::ChoiceOption => "choice_option",
        ContextualPosition::ImportAlias => "import_alias",
        ContextualPosition::ImportNamespace => "import_namespace",
        ContextualPosition::EachStatement => "each_statement",
    }
}

impl Server {
    fn invalid(&self, key: &str, args: &[(&'static str, String)]) -> ErrorData {
        let mut fluent = lingokit::FluentArgs::new();
        for (name, value) in args {
            fluent.set(*name, value.clone());
        }
        ErrorData::invalid_params(self.loc.fmt(key, Some(&fluent)), None)
    }

    fn sandbox_error(&self, e: &sandbox::Invalid) -> ErrorData {
        match e {
            sandbox::Invalid::TooLarge { limit } => {
                self.invalid("mcp-error-too-large", &[("limit", limit.to_string())])
            }
            sandbox::Invalid::ModuleName(n) => {
                self.invalid("mcp-error-module-name", &[("name", n.clone())])
            }
            sandbox::Invalid::NoEntry => self.invalid("mcp-error-no-entry", &[]),
            sandbox::Invalid::Duplicate(n) => {
                self.invalid("mcp-error-duplicate-module", &[("name", n.clone())])
            }
        }
    }

    /// Makes the modules of a script (name, source, display path) from the arguments.
    fn sources_of(
        &self,
        path: &Option<String>,
        source: &Option<String>,
        root: &Option<String>,
        extra: &Option<Vec<ModuleSource>>,
    ) -> Result<(Vec<modules::SourceFile>, usize), ErrorData> {
        match (path, source) {
            (Some(_), Some(_)) | (None, None) => Err(self.invalid("mcp-error-path-or-source", &[])),
            (Some(path), None) => {
                let root = root.as_deref().map(std::path::Path::new);
                modules::collect(std::path::Path::new(path), root).map_err(|errors| {
                    let reason = errors
                        .iter()
                        .map(|e| {
                            crate::texts::collect_error_line(
                                &self.loc,
                                std::path::Path::new(path),
                                e,
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.invalid("mcp-error-collect", &[("reason", reason)])
                })
            }
            (None, Some(source)) => {
                let mut files = vec![modules::SourceFile {
                    module: "main".into(),
                    path: "main".into(),
                    source: source.clone(),
                }];
                for m in extra.iter().flatten() {
                    files.push(modules::SourceFile {
                        module: m.name.clone(),
                        path: m.name.clone(),
                        source: m.source.clone(),
                    });
                }
                let request = sandbox::Request {
                    modules: files
                        .iter()
                        .map(|f| (f.module.clone(), f.source.clone()))
                        .collect(),
                    input: String::new(),
                    seed: None,
                    start: None,
                    test: None,
                };
                sandbox::validate(&request, &sandbox::Limits::default())
                    .map_err(|e| self.sandbox_error(&e))?;
                Ok((files, 0))
            }
        }
    }
}

#[tool_router(vis = "pub")]
impl Server {
    /// A new server.
    pub fn new(loc: Localizer) -> Self {
        let tool_router = lingokit::rmcp::localize_if_enabled(Self::tool_router(), &loc);
        Self { loc, tool_router }
    }

    /// Check a yomi script without running it and return the diagnostics (errors and warnings)
    #[tool(name = "check")]
    async fn check(
        &self,
        Parameters(p): Parameters<CheckParams>,
    ) -> Result<Json<CheckResult>, ErrorData> {
        let (files, entry) = self.sources_of(&p.path, &p.source, &p.root, &p.modules)?;
        let reg =
            crate::decls::registry_for(&self.loc, p.decls.as_deref().map(std::path::Path::new))
                .map_err(|reason| self.invalid("mcp-error-decls", &[("reason", reason)]))?;
        let sources = modules::sources(&files);
        let options = modules::options(&files, CompileOptions::default())
            .entry_label(&format!("{}.start", files[entry].module));
        let diags = match compile_modules_with_options(&sources, &reg, &options) {
            Ok((_, warnings)) => warnings,
            Err(diags) => diags,
        };
        let errors = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let diagnostics = diags
            .iter()
            .map(|d| DiagnosticItem {
                module: d.module.clone(),
                file: files
                    .iter()
                    .find(|f| f.module == d.module)
                    .map(|f| f.path.clone())
                    .unwrap_or_else(|| d.module.clone()),
                line: d.line,
                column: d.col,
                severity: severity_name(d.severity).into(),
                code: d.code.as_str().into(),
                message: diagnostic_message(&self.loc, d),
            })
            .collect();
        Ok(Json(CheckResult {
            ok: errors == 0,
            errors: errors as u32,
            warnings: (diags.len() - errors) as u32,
            diagnostics,
        }))
    }

    /// Format a yomi script in the standard form and return the result (nothing is written)
    #[tool(name = "format")]
    async fn format(
        &self,
        Parameters(p): Parameters<FormatParams>,
    ) -> Result<Json<FormatResult>, ErrorData> {
        let (files, entry) = self.sources_of(&p.path, &p.source, &p.root, &p.modules)?;
        let all = modules::all_sources(&files);
        let target = &files[entry];
        Ok(Json(match yomi_core::format_source(&all, &target.module) {
            Ok(text) => FormatResult {
                ok: true,
                changed: text != target.source,
                formatted: Some(text),
                diagnostics: Vec::new(),
            },
            Err(e) => {
                let diags = match e {
                    yomi_core::FormatError::Syntax(d) => d,
                    _ => Vec::new(),
                };
                FormatResult {
                    ok: false,
                    changed: false,
                    formatted: None,
                    diagnostics: diags
                        .iter()
                        .map(|d| DiagnosticItem {
                            module: d.module.clone(),
                            file: target.path.clone(),
                            line: d.line,
                            column: d.col,
                            severity: severity_name(d.severity).into(),
                            code: d.code.as_str().into(),
                            message: diagnostic_message(&self.loc, d),
                        })
                        .collect(),
                }
            }
        }))
    }

    /// List the reserved words of the yomi language and the words that have a meaning only in a fixed position
    #[tool(name = "reserved_words")]
    async fn reserved_words(&self) -> Result<Json<ReservedWords>, ErrorData> {
        Ok(Json(ReservedWords {
            keywords: KEYWORDS
                .iter()
                .map(|k| KeywordItem {
                    word: k.word.into(),
                    kind: crate::keyword::kind_name(k.kind).into(),
                    opens_block: k.opens_block,
                })
                .collect(),
            contextual: CONTEXTUAL_KEYWORDS
                .iter()
                .map(|k| ContextualItem {
                    word: k.word.into(),
                    position: position_name(k.position).into(),
                })
                .collect(),
        }))
    }

    /// List the diagnostic and error codes of yomi with their severity, arguments and messages
    #[tool(name = "diagnostic_codes")]
    async fn diagnostic_codes(
        &self,
        Parameters(p): Parameters<CodesParams>,
    ) -> Result<Json<CodesResult>, ErrorData> {
        let mut items: Vec<CodeItem> = all_codes()
            .iter()
            .map(|c| CodeItem {
                kind: "language".into(),
                group: c.group.name().into(),
                code: c.code.into(),
                severity: severity_name(c.severity).into(),
                args: c.args.iter().map(|a| a.to_string()).collect(),
                message: codes::describe(&self.loc, c),
            })
            .collect();
        if p.packages.unwrap_or(false) {
            items.extend(
                codes::command_codes()
                    .into_iter()
                    .map(|(kind, c)| CodeItem {
                        kind: kind.name().into(),
                        group: String::new(),
                        code: c.code.into(),
                        severity: "error".into(),
                        args: c.args.iter().map(|a| a.to_string()).collect(),
                        message: codes::command_message(&self.loc, c),
                    }),
            );
        }
        Ok(Json(CodesResult { codes: items }))
    }

    /// Read the language specification (Japanese Markdown); without `part` it lists the documents
    #[tool(name = "spec")]
    async fn spec(
        &self,
        Parameters(p): Parameters<SpecParams>,
    ) -> Result<Json<SpecResult>, ErrorData> {
        let parts = spec::PARTS
            .iter()
            .map(|(id, text)| SpecPart {
                id: (*id).into(),
                title: spec::title(text).into(),
            })
            .collect();
        let (id, text) = match p.part {
            None => (None, None),
            Some(id) => match spec::find(&id) {
                Some(text) => (Some(id), Some(text.to_string())),
                None => {
                    let known = spec::PARTS
                        .iter()
                        .map(|(i, _)| *i)
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(
                        self.invalid("mcp-error-unknown-part", &[("part", id), ("known", known)])
                    );
                }
            },
        };
        Ok(Json(SpecResult { parts, id, text }))
    }

    /// Run a yomi script in a sandbox (a child process with time, output and memory limits) in auto-play mode
    #[tool(name = "run")]
    async fn run(
        &self,
        Parameters(p): Parameters<RunParams>,
    ) -> Result<Json<RunResult>, ErrorData> {
        let mut modules = vec![("main".to_string(), p.source)];
        modules.extend(p.modules.into_iter().flatten().map(|m| (m.name, m.source)));
        let request = sandbox::Request {
            modules,
            input: p.input.unwrap_or_default(),
            seed: p.seed,
            start: p.start,
            test: None,
        };
        let limits = sandbox::Limits::default();
        sandbox::validate(&request, &limits).map_err(|e| self.sandbox_error(&e))?;
        let exe = std::env::current_exe()
            .map_err(|e| self.invalid("mcp-error-spawn", &[("reason", e.to_string())]))?;
        let out = sandbox::run(&exe, &request, &limits)
            .await
            .map_err(|e| self.invalid("mcp-error-spawn", &[("reason", e.to_string())]))?;
        Ok(Json(RunResult {
            stdout: out.stdout,
            stderr: out.stderr,
            exit_code: out.exit_code,
            timed_out: out.timed_out,
            truncated: out.truncated,
        }))
    }

    /// Explore the branches of a yomi script in a sandbox and report the error paths, the stops that cannot be answered and what was not reached
    #[tool(name = "test")]
    async fn test(
        &self,
        Parameters(p): Parameters<TestParams>,
    ) -> Result<Json<TestResult>, ErrorData> {
        let seeds = p.seeds.unwrap_or(1);
        if seeds == 0 {
            return Err(self.invalid("mcp-error-seeds", &[]));
        }
        let max_states = p.max_states.unwrap_or(TEST_MAX_STATES_DEFAULT);
        if max_states == 0 || max_states > TEST_MAX_STATES_LIMIT {
            return Err(self.invalid(
                "mcp-error-max-states",
                &[("limit", TEST_MAX_STATES_LIMIT.to_string())],
            ));
        }
        let mut events = Vec::new();
        for (name, list) in p.events.into_iter().flatten() {
            if name.is_empty() || name.contains('=') {
                return Err(self.invalid("mcp-error-event-name", &[("name", name)]));
            }
            events.extend(list.into_iter().map(|d| (name.clone(), d)));
        }
        let mut modules = vec![("main".to_string(), p.source)];
        modules.extend(p.modules.into_iter().flatten().map(|m| (m.name, m.source)));
        let request = sandbox::Request {
            modules,
            input: String::new(),
            seed: None,
            start: p.start,
            test: Some(sandbox::TestArgs {
                seeds,
                max_states,
                max_steps: p.max_steps,
                events,
            }),
        };
        let limits = sandbox::Limits {
            timeout: TEST_TIMEOUT,
            ..sandbox::Limits::default()
        };
        sandbox::validate(&request, &limits).map_err(|e| self.sandbox_error(&e))?;
        let exe = std::env::current_exe()
            .map_err(|e| self.invalid("mcp-error-spawn", &[("reason", e.to_string())]))?;
        let out = sandbox::run(&exe, &request, &limits)
            .await
            .map_err(|e| self.invalid("mcp-error-spawn", &[("reason", e.to_string())]))?;
        let report = serde_json::from_str::<serde_json::Value>(&out.stdout)
            .ok()
            .map(|mut v| {
                if let Some(o) = v.as_object_mut() {
                    o.remove("golden");
                }
                v
            });
        Ok(Json(TestResult {
            report,
            stderr: out.stderr,
            exit_code: out.exit_code,
            timed_out: out.timed_out,
            truncated: out.truncated,
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("yomi", yomi_core::version()))
            .with_instructions(lingokit::rmcp::instructions(
                &self.loc,
                "Tools for the yomi scripting language: check a script (check), list the reserved words (reserved_words) and the diagnostic codes (diagnostic_codes), read the specification and the guide (spec), run a script in a sandbox (run), and explore its branches (test).",
            ))
    }
}

/// `yomi mcp`. Runs over standard input and output until the client closes.
pub fn run(loc: Localizer) -> i32 {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let result = runtime.block_on(async {
        let service = Server::new(loc)
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| e.to_string())?;
        service.waiting().await.map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    });
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests;
