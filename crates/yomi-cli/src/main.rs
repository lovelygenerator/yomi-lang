//! The `yomi` command. Input and output with the terminal are confined to this file.

#![forbid(unsafe_code)]

mod assign_ids;
mod codes;
mod dap;
mod debugcmd;
mod decls;
mod diag;
mod fmt;
#[cfg(any(feature = "lsp", feature = "mcp"))]
mod keyword;
#[cfg(feature = "lsp")]
mod lsp;
#[cfg(feature = "mcp")]
mod mcp;
mod modules;
mod release;
mod repl;
mod schema;
mod session;
mod std_errors;
mod testcmd;
mod texts;
mod watch;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

lingokit::include_locales!();

/// Command line arguments.
#[derive(Debug, Parser)]
#[command(name = "yomi", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Subcommands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Prints the version.
    Version,
    /// Runs a script.
    Run {
        /// The script file to run (the entry file).
        file: std::path::PathBuf,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The label to start from.
        #[arg(long)]
        start: Option<String>,
        /// The file for persistent data (persistent vars and passed stop points).
        #[arg(long)]
        persistent: Option<std::path::PathBuf>,
        /// The random seed.
        #[arg(long)]
        seed: Option<u64>,
        /// The file of the text table that replaces bodies.
        #[arg(long)]
        texts: Option<std::path::PathBuf>,
        /// Automatic play. Does not read input for bodies and waits; reads only choices and input from standard input.
        #[arg(long)]
        auto: bool,
        /// Shows the stable IDs of options.
        #[arg(long)]
        show_ids: bool,
        /// Resets variables whose type changed to their initial values when loading, instead of failing.
        #[arg(long)]
        lenient: bool,
        /// Watches the scripts and the text table, and reloads them when they change.
        #[arg(long, conflicts_with = "auto")]
        watch: bool,
    },
    /// Runs a Debug Adapter Protocol server over standard input and output.
    Dap,
    /// Runs a script a source line at a time, with breakpoints and inspection.
    Debug {
        /// The script file to debug (the entry file).
        file: std::path::PathBuf,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The label to start from.
        #[arg(long)]
        start: Option<String>,
        /// The random seed.
        #[arg(long)]
        seed: Option<u64>,
        /// The file of the text table that replaces bodies.
        #[arg(long)]
        texts: Option<std::path::PathBuf>,
    },
    /// Explores the branches of a script and compares what each path shows with golden files.
    Test {
        /// The script file to test (the entry file).
        file: std::path::PathBuf,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The label to start from.
        #[arg(long)]
        start: Option<String>,
        /// Explores with the random seeds 0 to this number minus 1.
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
        seeds: u64,
        /// The maximum number of states to examine.
        #[arg(long, default_value_t = yomi_core::DEFAULT_MAX_STATES)]
        max_states: usize,
        /// The maximum number of instructions of one run.
        #[arg(long)]
        max_steps: Option<u64>,
        /// The data of an event to try at `event.wait` (`NAME=DATA`; repeatable).
        #[arg(long = "event", value_name = "NAME=DATA", value_parser = parse_event)]
        events: Vec<(String, String)>,
        /// The directory of golden files.
        #[arg(long)]
        golden: Option<std::path::PathBuf>,
        /// Rewrites the golden files instead of comparing.
        #[arg(long)]
        update: bool,
        /// Also fails when some stop point, option or label was never reached, or the search was cut off.
        #[arg(long)]
        require_coverage: bool,
        /// Prints the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// An interactive environment (REPL). Tries expressions, statements, declarations and script lines on the spot.
    Repl {
        /// The entry file. If absent, starts with an empty environment without a script.
        file: Option<std::path::PathBuf>,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The random seed.
        #[arg(long)]
        seed: Option<u64>,
        /// The file for persistent data (persistent vars and passed stop points). Only read, never written back.
        #[arg(long)]
        persistent: Option<std::path::PathBuf>,
        /// Resets variables whose type changed to their initial values when loading, instead of failing.
        #[arg(long)]
        lenient: bool,
    },
    /// Writes the declaration file (JSON) of the built-in registry (the standard packages) to standard output.
    Decls,
    /// Checks scripts without the host's Rust code (does not run them).
    Check {
        /// The script file to check (the entry file).
        file: std::path::PathBuf,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The declaration file (the JSON written by `yomi decls`). If absent, checks with the built-in registry.
        #[arg(long)]
        decls: Option<std::path::PathBuf>,
    },
    /// Appends automatic IDs to lines without a stable ID.
    AssignIds {
        /// The script files to rewrite (for a directory, all `.yomi` under it).
        #[arg(required = true, num_args = 1..)]
        paths: Vec<std::path::PathBuf>,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The declaration file.
        #[arg(long)]
        decls: Option<std::path::PathBuf>,
        /// The seed of the random generator for IDs.
        #[arg(long)]
        seed: Option<u64>,
        /// If there are lines without IDs, exits with code 1 without writing.
        #[arg(long)]
        check: bool,
        /// Shows the IDs to add and their positions without writing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Formats scripts in the standard form.
    Fmt {
        /// The script files (for a directory, all `.yomi` under it), or `-` for the standard input.
        #[arg(required = true, num_args = 1..)]
        paths: Vec<std::path::PathBuf>,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// Does not write; lists the files that need formatting and exits with code 1 if there are any.
        #[arg(long)]
        check: bool,
        /// Prints the result instead of rewriting the file.
        #[arg(long)]
        stdout: bool,
        /// The path of the file the standard input stands for.
        #[arg(long)]
        stdin_path: Option<std::path::PathBuf>,
    },
    /// Shows the list of codes (diagnostics, runtime errors and so on).
    Codes {
        /// Prints in machine-readable form (JSON).
        #[arg(long)]
        json: bool,
        /// Also shows the error codes of standard package and core commands.
        #[arg(long)]
        packages: bool,
    },
    /// Runs the Language Server over standard input and output (for editors).
    #[cfg(feature = "lsp")]
    Lsp {
        /// The declaration file (the JSON written by `yomi decls`). If absent, uses the built-in registry.
        #[arg(long)]
        decls: Option<std::path::PathBuf>,
    },
    /// Runs the MCP server over standard input and output.
    #[cfg(feature = "mcp")]
    Mcp,
    /// Tools for text tables that replace bodies (localization).
    #[command(disable_help_subcommand = true)]
    Texts {
        #[command(subcommand)]
        action: TextsCommand,
    },
    /// Tools for the schema of saves (freezing it at a release).
    #[command(disable_help_subcommand = true)]
    Schema {
        #[command(subcommand)]
        action: SchemaCommand,
    },
    /// Checks the scripts strictly and freezes the schema (run it before a release).
    Release {
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The declaration file.
        #[arg(long, conflicts_with = "saves")]
        decls: Option<std::path::PathBuf>,
        /// Saves and persistent data (files or directories) to check loading of.
        #[arg(long, num_args = 1..)]
        saves: Vec<std::path::PathBuf>,
        /// Only checks; does not write the frozen schema.
        #[arg(long)]
        dry_run: bool,
    },
    /// Makes a template of a `migrate` block from the latest frozen schema.
    MigrateInit {
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The declaration file.
        #[arg(long)]
        decls: Option<std::path::PathBuf>,
        /// Appends the template to `migrations.yomi` instead of printing it.
        #[arg(long)]
        write: bool,
    },
}

/// Subcommands of `yomi schema`.
#[derive(Debug, Subcommand)]
enum SchemaCommand {
    /// Writes the schema of the current scripts to `schema/v<version>.yomi`.
    Freeze {
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The declaration file.
        #[arg(long)]
        decls: Option<std::path::PathBuf>,
    },
    /// Rebuilds the content of a frozen schema from the schema inside a save (and persistent data).
    Extract {
        /// The save file.
        save: std::path::PathBuf,
        /// The persistent data file.
        persistent: Option<std::path::PathBuf>,
        /// The file to write (standard output if absent).
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
}

/// Subcommands of `yomi texts`.
#[derive(Debug, Subcommand)]
enum TextsCommand {
    /// Checks scripts against a text table (missing translations, extra translations, number errors).
    Check {
        /// The script file (the entry file).
        script: std::path::PathBuf,
        /// The text table file.
        table: std::path::PathBuf,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
    },
    /// Writes to standard output a template text table whose values are the originals, from the body lines of the scripts.
    Template {
        /// The script file (the entry file).
        script: std::path::PathBuf,
        /// The directory to search for modules.
        #[arg(long)]
        root: Option<std::path::PathBuf>,
        /// The language name of the bodies written in the template (the `@lang` line).
        #[arg(long)]
        text_language: Option<String>,
    },
}

/// Runs the parsed command line and returns the exit code.
fn run(cli: Cli, loc: &lingokit::Localizer) -> ExitCode {
    match cli.command {
        Command::Version => {
            println!("{}", loc.msg(&msg::version_line(yomi_core::version())));
            ExitCode::SUCCESS
        }
        Command::Run {
            file,
            root,
            start,
            persistent,
            seed,
            texts,
            auto,
            show_ids,
            lenient,
            watch,
        } => run_command(
            loc,
            &file,
            root.as_deref(),
            start,
            watch,
            session::RunOptions {
                persistent,
                seed,
                texts,
                auto,
                show_ids,
                lenient,
                interactive: io::stdin().is_terminal(),
                entry: Some(file.clone()),
                root: root.clone(),
            },
        ),
        Command::Dap => finish(dap::run(loc, io::stdin(), io::stdout())),
        Command::Debug {
            file,
            root,
            start,
            seed,
            texts,
        } => debug_command(
            loc,
            &file,
            root.as_deref(),
            start,
            &debugcmd::Options {
                seed,
                texts,
                interactive: io::stdin().is_terminal(),
            },
        ),
        Command::Test {
            file,
            root,
            start,
            seeds,
            max_states,
            max_steps,
            events,
            golden,
            update,
            require_coverage,
            json,
        } => finish(testcmd::run(
            loc,
            &testcmd::Options {
                file,
                root,
                start,
                seeds,
                max_states,
                max_steps,
                events,
                golden,
                update,
                require_coverage,
                json,
            },
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )),
        Command::Repl {
            file,
            root,
            seed,
            persistent,
            lenient,
        } => repl_command(
            loc,
            file.as_deref(),
            root.as_deref(),
            seed,
            persistent,
            lenient,
        ),
        #[cfg(feature = "lsp")]
        Command::Lsp { decls } => {
            let registry = match decls::registry_for(loc, decls.as_deref()) {
                Ok(r) => r,
                Err(line) => {
                    eprintln!("{line}");
                    return ExitCode::FAILURE;
                }
            };
            // The display language is decided from the environment (editors do not necessarily pass `--lang`)
            finish(lsp::run(
                lingokit::Localizer::from_env(&LOCALES, None),
                registry,
            ))
        }
        #[cfg(feature = "mcp")]
        Command::Mcp => {
            // MCP has no way to send a language, so the display language is decided from the environment (`--lang` has no effect)
            finish(mcp::run(lingokit::Localizer::from_env(&LOCALES, None)))
        }
        Command::Texts { action } => texts_command(loc, action),
        Command::Schema { action } => match action {
            SchemaCommand::Freeze { root, decls } => finish(schema::freeze(
                loc,
                &schema::ProjectOptions {
                    root,
                    decls,
                    write: false,
                },
                &mut io::stdout().lock(),
                &mut io::stderr().lock(),
            )),
            SchemaCommand::Extract {
                save,
                persistent,
                out,
            } => finish(schema::extract(
                loc,
                &save,
                persistent.as_deref(),
                out.as_deref(),
                &mut io::stdout().lock(),
                &mut io::stderr().lock(),
            )),
        },
        Command::Release {
            root,
            decls,
            saves,
            dry_run,
        } => finish(release::run(
            loc,
            &release::Options {
                root,
                decls,
                saves,
                dry_run,
            },
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )),
        Command::MigrateInit { root, decls, write } => finish(schema::migrate_init(
            loc,
            &schema::ProjectOptions { root, decls, write },
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )),
        Command::AssignIds {
            paths,
            root,
            decls,
            seed,
            check,
            dry_run,
        } => finish(assign_ids::run(
            loc,
            &assign_ids::Options {
                paths,
                root,
                decls,
                seed,
                check,
                dry_run,
            },
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )),
        Command::Fmt {
            paths,
            root,
            check,
            stdout,
            stdin_path,
        } => finish(fmt::run(
            loc,
            &fmt::Options {
                paths,
                root,
                check,
                stdout,
                stdin_path,
            },
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )),
        Command::Codes { json, packages } => {
            finish(codes::run(loc, json, packages, &mut io::stdout().lock()))
        }
        Command::Decls => finish(decls::export(&mut io::stdout().lock())),
        Command::Check { file, root, decls } => finish(decls::check(
            loc,
            &file,
            root.as_deref(),
            decls.as_deref(),
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        )),
    }
}

/// Turns the exit code (0 for success) into the process's exit code.
fn finish(code: i32) -> ExitCode {
    if code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `yomi texts`.
fn texts_command(loc: &lingokit::Localizer, action: TextsCommand) -> ExitCode {
    let mut stdout = io::stdout().lock();
    let code = match action {
        TextsCommand::Check {
            script,
            table,
            root,
        } => texts::check(
            loc,
            &script,
            &table,
            root.as_deref(),
            &mut stdout,
            &mut io::stderr().lock(),
        ),
        TextsCommand::Template {
            script,
            root,
            text_language,
        } => texts::template(
            loc,
            &script,
            root.as_deref(),
            text_language.as_deref().unwrap_or(""),
            &mut stdout,
            &mut io::stderr().lock(),
        ),
    };
    let _ = stdout.flush();
    if code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `yomi run`. Collects the entry file and the modules under the same directory (changeable with `--root`) and runs them.
fn run_command(
    loc: &lingokit::Localizer,
    file: &std::path::Path,
    root: Option<&std::path::Path>,
    start: Option<String>,
    watch: bool,
    options: session::RunOptions,
) -> ExitCode {
    let name = file.display().to_string();
    let (files, entry) = match modules::collect(file, root) {
        Ok(found) => found,
        Err(errors) => {
            for e in &errors {
                eprintln!("{}", texts::collect_error_line(loc, &name, e));
            }
            return ExitCode::FAILURE;
        }
    };
    // The start is `module.label`. A plain label name refers to the entry file's module
    let start = modules::start_label(&files, entry, start);
    let mut stdout = io::stdout().lock();
    let code = if watch {
        let mut input = match watch::WatchInput::start(file, root, options.texts.as_deref()) {
            Ok(input) => input,
            Err(reason) => {
                eprintln!("{}", diag::error_line(loc, &name, &reason));
                return ExitCode::FAILURE;
            }
        };
        session::run_script(
            loc,
            &files,
            &start,
            &options,
            &mut input,
            &mut stdout,
            &mut io::stderr().lock(),
        )
    } else {
        let stdin = io::stdin();
        session::run_script(
            loc,
            &files,
            &start,
            &options,
            &mut stdin.lock(),
            &mut stdout,
            &mut io::stderr().lock(),
        )
    };
    let _ = stdout.flush();
    if code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// `yomi debug`.
fn debug_command(
    loc: &lingokit::Localizer,
    file: &std::path::Path,
    root: Option<&std::path::Path>,
    start: Option<String>,
    options: &debugcmd::Options,
) -> ExitCode {
    let name = file.display().to_string();
    let (files, entry) = match modules::collect(file, root) {
        Ok(found) => found,
        Err(errors) => {
            for e in &errors {
                eprintln!("{}", texts::collect_error_line(loc, &name, e));
            }
            return ExitCode::FAILURE;
        }
    };
    let start = modules::start_label(&files, entry, start);
    let mut stdout = io::stdout().lock();
    let code = debugcmd::run_script(
        loc,
        &files,
        &start,
        options,
        &mut io::stdin().lock(),
        &mut stdout,
        &mut io::stderr().lock(),
    );
    let _ = stdout.flush();
    finish(code)
}

/// `yomi repl`. If there is a script, collects and compiles it and runs on the environment at its start point.
fn repl_command(
    loc: &lingokit::Localizer,
    file: Option<&std::path::Path>,
    root: Option<&std::path::Path>,
    seed: Option<u64>,
    persistent: Option<std::path::PathBuf>,
    lenient: bool,
) -> ExitCode {
    let collected = match file {
        Some(file) => match modules::collect(file, root) {
            Ok(found) => Some((found, file.display().to_string())),
            Err(errors) => {
                let name = file.display().to_string();
                for e in &errors {
                    eprintln!("{}", texts::collect_error_line(loc, &name, e));
                }
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let options = repl::ReplOptions {
        persistent,
        seed,
        lenient,
        interactive: io::stdin().is_terminal(),
    };
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    let script = collected
        .as_ref()
        .map(|((files, entry), _)| (files.as_slice(), files[*entry].module.as_str()));
    let code = repl::run_repl(
        loc,
        script,
        &options,
        &mut stdin.lock(),
        &mut stdout,
        &mut io::stderr().lock(),
    );
    let _ = stdout.flush();
    if code == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A mark for printing `{n}`, which clap interprets (as a newline), as literal characters in help strings.
/// It is placed between `{` and `n` in FTL and removed before printing help (clap unconditionally replaces `{n}` with a
/// newline).
const HELP_LITERAL_MARK: char = '\u{200B}';

/// Reads the arguments. Help is printed after restoring the `{n}` that clap would turn into newlines. Everything else is
/// left to lingokit.
fn parse_args() -> (Cli, lingokit::Localizer) {
    use clap::FromArgMatches;
    use clap::error::ErrorKind;
    use std::io::IsTerminal;

    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let loc = lingokit::Localizer::from_env(
        &LOCALES,
        lingokit::locale::pre_scan_lang(args.clone()).as_deref(),
    );
    let cmd = lingokit::clap::command::<Cli>(&loc);
    let matches = match cmd.try_get_matches_from(args) {
        Ok(m) => m,
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) =>
        {
            let to_stderr = e.use_stderr();
            let color = if to_stderr {
                io::stderr().is_terminal()
            } else {
                io::stdout().is_terminal()
            } && std::env::var_os("NO_COLOR").is_none();
            let rendered = e.render();
            let text = if color {
                rendered.ansi().to_string()
            } else {
                rendered.to_string()
            };
            let text = text.replace(HELP_LITERAL_MARK, "");
            if to_stderr {
                eprint!("{text}");
            } else {
                print!("{text}");
                let _ = io::stdout().flush();
            }
            std::process::exit(e.exit_code());
        }
        Err(e) => lingokit::clap::exit_with_clap_error(e, &loc),
    };
    let cli = Cli::from_arg_matches(&matches)
        .unwrap_or_else(|e| lingokit::clap::exit_with_clap_error(e, &loc));
    (cli, loc)
}

fn main() -> ExitCode {
    let (cli, loc) = parse_args();
    run(cli, &loc)
}

/// Parses `NAME=DATA` of `yomi test --event` (split at the first `=`; the name cannot be empty).
fn parse_event(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((name, data)) if !name.is_empty() => Ok((name.to_string(), data.to_string())),
        _ => Err("expected NAME=DATA".to_string()),
    }
}

#[cfg(test)]
mod tests;
