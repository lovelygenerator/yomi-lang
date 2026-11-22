//! Collects the modules (`.yomi` files) that `yomi run` reads.
//!
//! The name of a module is its path relative to the search directory, without the extension and with separators replaced
//! by `::` (`story/ch1.yomi` is `story::ch1`; section 4.1 of `spec/language/04-labels-modules.md`).
//! The core does not read files, so collecting them is done here.

use std::fs;
use std::path::{Path, PathBuf};

/// The source of a collected module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    /// The name of the module (the path from the search directory; `story::ch1`).
    pub module: String,
    /// The path of the file used for display.
    pub path: String,
    /// The source.
    pub source: String,
}

/// The name `migrations.yomi` gets as a collected file.
pub(crate) const MIGRATIONS: &str = "migrations";

impl SourceFile {
    /// Whether this is `migrations.yomi` (an input of save migration, not a module).
    pub(crate) fn is_migrations(&self) -> bool {
        self.module == MIGRATIONS
    }

    /// Whether this is a frozen schema (`schema/vN.yomi`; also not a module).
    pub(crate) fn is_frozen_schema(&self) -> bool {
        self.module.starts_with("schema::")
    }

    fn is_module(&self) -> bool {
        !self.is_migrations() && !self.is_frozen_schema()
    }
}

/// The list of `(module name, source)` passed to the compiler (without `migrations.yomi` and the frozen schemas).
pub(crate) fn sources(files: &[SourceFile]) -> Vec<(&str, &str)> {
    files
        .iter()
        .filter(|f| f.is_module())
        .map(|f| (f.module.as_str(), f.source.as_str()))
        .collect()
}

/// Every collected file as `(name, source)` (what the formatter needs to classify lines).
pub(crate) fn all_sources(files: &[SourceFile]) -> Vec<(&str, &str)> {
    files
        .iter()
        .map(|f| (f.module.as_str(), f.source.as_str()))
        .collect()
}

/// Adds `migrations.yomi` and the frozen schemas among `files` to the compile options.
pub(crate) fn options(
    files: &[SourceFile],
    mut options: yomi_core::CompileOptions,
) -> yomi_core::CompileOptions {
    for f in files {
        if f.is_migrations() {
            options = options.migrations(&f.source);
        } else if f.is_frozen_schema() {
            let rest = f.module.trim_start_matches("schema::").replace("::", "/");
            options = options.frozen_schema_file(&format!("schema/{rest}.yomi"), &f.source);
        }
    }
    options
}

/// The path of a module's file, used to display diagnostics. If the module is not found, the path of the first file.
pub(crate) fn path_of(files: &[SourceFile], module: &str) -> String {
    files
        .iter()
        .find(|f| f.module == module)
        .or_else(|| files.first())
        .map(|f| f.path.clone())
        .unwrap_or_default()
}

/// The reason they could not be collected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectError {
    /// A file or directory cannot be read.
    Read { path: String, reason: String },
    /// The entry file is not under the search directory.
    EntryOutsideRoot { path: String, root: String },
}

/// The start label as `module.label`: a name with a `.` is used as it is, a plain label name refers to the module of the entry
/// file, and without one it is the `start` label of that module.
pub(crate) fn start_label(files: &[SourceFile], entry: usize, start: Option<String>) -> String {
    let entry_module = &files[entry].module;
    match start {
        Some(s) if s.contains('.') => s,
        Some(s) => format!("{entry_module}.{s}"),
        None => format!("{entry_module}.start"),
    }
}

/// Collects the entry file and the `.yomi` under the search directory (`root`; if absent, the directory of the entry
/// file). Returns the modules (in path order) and the index of the entry file.
///
/// If the entry file cannot be read, fails without searching for others.
pub fn collect(
    entry: &Path,
    root: Option<&Path>,
) -> Result<(Vec<SourceFile>, usize), Vec<CollectError>> {
    let read = |path: &Path| {
        fs::read_to_string(path).map_err(|e| CollectError::Read {
            path: path.display().to_string(),
            reason: e.to_string(),
        })
    };
    let entry_source = read(entry).map_err(|e| vec![e])?;
    let root: PathBuf = match root {
        Some(r) => r.to_path_buf(),
        None => match entry.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    let mut paths = Vec::new();
    let entry_canonical = fs::canonicalize(entry).ok();
    walk(&root, &mut paths, entry_canonical.as_deref()).map_err(|e| vec![e])?;
    paths.sort();

    let mut errors = Vec::new();
    let mut files: Vec<SourceFile> = Vec::new();
    let mut entry_index = None;
    for path in paths {
        let is_entry = entry_canonical.is_some() && fs::canonicalize(&path).ok() == entry_canonical;
        // The entry file is displayed with the path exactly as given
        let shown = if is_entry {
            entry.display().to_string()
        } else {
            path.display().to_string()
        };
        let module = module_name(&path, &root);
        let source = if is_entry {
            entry_source.clone()
        } else {
            match read(&path) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            }
        };
        if is_entry {
            entry_index = Some(files.len());
        }
        files.push(SourceFile {
            module,
            path: shown,
            source,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    match entry_index {
        Some(i) => Ok((files, i)),
        None => Err(vec![CollectError::EntryOutsideRoot {
            path: entry.display().to_string(),
            root: root.display().to_string(),
        }]),
    }
}

/// Collects every `.yomi` under the directory `root` (modules, `migrations.yomi` and the frozen schemas).
pub fn collect_dir(root: &Path) -> Result<Vec<SourceFile>, Vec<CollectError>> {
    let paths = scripts_under(root).map_err(|e| vec![e])?;
    let mut files = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        match fs::read_to_string(&path) {
            Ok(source) => files.push(SourceFile {
                module: module_name(&path, root),
                path: path.display().to_string(),
                source,
            }),
            Err(e) => errors.push(CollectError::Read {
                path: path.display().to_string(),
                reason: e.to_string(),
            }),
        }
    }
    if errors.is_empty() {
        Ok(files)
    } else {
        Err(errors)
    }
}

/// Makes a module name from the path of a `.yomi`. Joins with `::` each component of the path relative to `root`,
/// without the extension (only the file name if it is not under `root`).
pub(crate) fn module_name(path: &Path, root: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut parts: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if let Some(last) = parts.last_mut()
        && let Some(stem) = Path::new(last.as_str()).file_stem()
    {
        *last = stem.to_string_lossy().into_owned();
    }
    parts.join("::")
}

/// The paths of the `.yomi` under a directory (in path order).
pub fn scripts_under(dir: &Path) -> Result<Vec<PathBuf>, CollectError> {
    let mut out = Vec::new();
    walk(dir, &mut out, None)?;
    out.sort();
    Ok(out)
}

/// Collects the paths of the `.yomi` under a directory recursively (directories that are symbolic links are not
/// followed).
///
/// Directories and files whose names start with `.` (`.git`, editor temporary files and so on) are skipped. However,
/// `keep` (the normalized path of the entry file) is collected even if it is a hidden file or under a hidden directory.
fn walk(dir: &Path, out: &mut Vec<PathBuf>, keep: Option<&Path>) -> Result<(), CollectError> {
    let fail = |path: &Path, e: std::io::Error| CollectError::Read {
        path: path.display().to_string(),
        reason: e.to_string(),
    };
    for e in fs::read_dir(dir).map_err(|e| fail(dir, e))? {
        let e = e.map_err(|e| fail(dir, e))?;
        let path = e.path();
        let kind = e.file_type().map_err(|e| fail(&path, e))?;
        let hidden = e.file_name().to_string_lossy().starts_with('.');
        let canonical = || fs::canonicalize(&path).ok();
        if kind.is_dir() {
            if hidden && !keep.is_some_and(|k| canonical().is_some_and(|c| k.starts_with(c))) {
                continue;
            }
            walk(&path, out, keep)?;
        } else if hidden && !keep.is_some_and(|k| canonical().as_deref() == Some(k)) {
            continue;
        } else if path.extension().is_some_and(|x| x == "yomi")
            && (kind.is_file() || fs::metadata(&path).is_ok_and(|m| m.is_file()))
        {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
