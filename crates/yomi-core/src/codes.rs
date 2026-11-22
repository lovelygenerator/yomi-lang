//! The list of codes (used by `yomi codes` and to check against the list in `spec/language/06-diagnostics.md`).
//!
//! Lists the stable English names and argument names of compile diagnostics ([`DiagCode`]), as well as runtime errors,
//! load errors, text table errors and check results, and declaration file errors.

use crate::diag::{DiagCode, Severity};

/// A group of codes (corresponds to the prefix of the FTL keys used to look up messages).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeGroup {
    /// Compile diagnostics (`diag-`).
    Diag,
    /// Runtime errors (`vm-`).
    Vm,
    /// Load errors (`load-`).
    Load,
    /// Text table errors (`text-error-`).
    TextError,
    /// Results of checking a text table against the scripts (`text-issue-`).
    TextIssue,
    /// Declaration file errors (`decl-error-`).
    DeclError,
}

impl CodeGroup {
    /// All groups.
    pub const ALL: &'static [CodeGroup] = &[
        CodeGroup::Diag,
        CodeGroup::Vm,
        CodeGroup::Load,
        CodeGroup::TextError,
        CodeGroup::TextIssue,
        CodeGroup::DeclError,
    ];

    /// The name of the group (`group` in `yomi codes --json`).
    pub fn name(self) -> &'static str {
        match self {
            CodeGroup::Diag => "diag",
            CodeGroup::Vm => "vm",
            CodeGroup::Load => "load",
            CodeGroup::TextError => "text-error",
            CodeGroup::TextIssue => "text-issue",
            CodeGroup::DeclError => "decl-error",
        }
    }

    /// The prefix of the message (FTL) keys (such as `diag-`; the code follows it).
    pub fn ftl_prefix(self) -> &'static str {
        match self {
            CodeGroup::Diag => "diag-",
            CodeGroup::Vm => "vm-",
            CodeGroup::Load => "load-",
            CodeGroup::TextError => "text-error-",
            CodeGroup::TextIssue => "text-issue-",
            CodeGroup::DeclError => "decl-error-",
        }
    }
}

/// The public specification of one code.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeInfo {
    /// Group.
    pub group: CodeGroup,
    /// The stable English name.
    pub code: &'static str,
    /// Argument names (`suggestions` of a diagnostic is present only when there are close names).
    pub args: &'static [&'static str],
    /// Severity.
    pub severity: Severity,
}

/// The code and arguments of a runtime error ([`crate::VmErrorKind`]).
pub(crate) const VM_CODES: &[(&str, &[&str])] = &[
    ("step_limit_exceeded", &[]),
    ("interrupted", &[]),
    ("call_depth_exceeded", &[]),
    ("value_too_large", &[]),
    ("memory_limit_exceeded", &[]),
    ("index_out_of_range", &["index", "len"]),
    ("key_not_found", &["key"]),
    ("integer_overflow", &[]),
    ("division_by_zero", &[]),
    ("non_finite_float", &[]),
    ("float_out_of_range", &[]),
    ("not_stopped", &[]),
    ("already_stopped", &[]),
    ("ended", &[]),
    ("no_choice_available", &[]),
    ("choice_out_of_range", &["index", "count"]),
    ("poisoned", &[]),
    ("persistent_data", &["reason"]),
    ("invalid_random_range", &["min", "max"]),
    ("snapshot_mismatch", &[]),
    ("bad_input", &["expected", "found"]),
    ("start_label_not_found", &["name"]),
    ("command_missing", &["name"]),
    ("command_failed", &["code"]),
    ("unknown_variable", &["name"]),
    ("slice_serialize", &["name"]),
    ("decl_only_registry", &[]),
];

/// One error code of a command (decided by the package; [`crate::CommandError::with_code`]).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ErrorCodeInfo {
    /// The stable English name, in the form `package.error_name`.
    pub code: &'static str,
    /// Argument names.
    pub args: &'static [&'static str],
}

impl ErrorCodeInfo {
    /// Makes one from a code and argument names (used when writing a package's list of codes).
    pub const fn new(code: &'static str, args: &'static [&'static str]) -> Self {
        Self { code, args }
    }
}

/// Codes that the core returns as `CommandFailed` (errors in the core's mechanisms; bugs in the host's implementation).
const CORE_COMMAND_ERRORS: &[ErrorCodeInfo] = &[
    ErrorCodeInfo::new("core.state_type_mismatch", &["package"]),
    ErrorCodeInfo::new("core.persistent_type_mismatch", &["package"]),
    ErrorCodeInfo::new("core.dep_not_declared", &["package", "dep"]),
    ErrorCodeInfo::new("core.dep_not_registered", &["dep"]),
    ErrorCodeInfo::new("core.dep_is_self", &["package"]),
    ErrorCodeInfo::new("core.bad_args", &["name"]),
    ErrorCodeInfo::new("core.package_slice_missing", &[]),
    ErrorCodeInfo::new("core.decl_only_function", &[]),
];

/// The list of codes the core returns as command failures (`VmErrorKind::CommandFailed`; codes start with `core.`).
pub fn core_command_error_codes() -> &'static [ErrorCodeInfo] {
    CORE_COMMAND_ERRORS
}

/// The code and arguments of a load error ([`crate::LoadErrorKind`]).
pub(crate) const LOAD_CODES: &[(&str, &[&str])] = &[
    ("bad_magic", &[]),
    ("unsupported_version", &["version"]),
    ("checksum_mismatch", &[]),
    ("corrupt", &[]),
    ("label_not_found", &["name"]),
    ("module_mismatch", &["name"]),
    ("command_missing", &["name"]),
    ("stop_args_failed", &[]),
    ("package_slice", &["name"]),
    ("decl_only_registry", &[]),
    ("save_from_newer_version", &["saved", "current"]),
    ("save_version_unsupported", &["saved", "min"]),
    ("schema_mismatch", &["vars"]),
    ("migrate_failed", &["vars"]),
];

/// The code and arguments of a text table error ([`crate::TextErrorKind`]).
pub(crate) const TEXT_ERROR_CODES: &[(&str, &[&str])] = &[
    ("invalid_escape", &["found", "key"]),
    ("invalid_placeholder", &["found", "key"]),
    ("missing_tab", &[]),
    ("empty_key", &[]),
    ("duplicate_key", &["key"]),
    ("unknown_directive", &["name"]),
];

/// The code, arguments and severity of a check result ([`crate::TextIssueKind`]).
pub(crate) const TEXT_ISSUE_CODES: &[(&str, &[&str], Severity)] = &[
    ("missing", &["key"], Severity::Warning),
    ("unused", &["key"], Severity::Warning),
    ("bad_number", &["key", "number", "count"], Severity::Error),
];

/// The code and arguments of a declaration file error ([`crate::DeclErrorKind`]).
pub(crate) const DECL_ERROR_CODES: &[(&str, &[&str])] = &[
    ("syntax", &["line", "col"]),
    ("too_large", &[]),
    ("too_deep", &[]),
    ("not_decls", &[]),
    ("unsupported_version", &["version"]),
    ("missing_field", &[]),
    ("wrong_type", &[]),
    ("invalid_type", &["found"]),
    ("invalid_value", &[]),
    ("registry", &["reason"]),
];

/// The list of all codes (in group order; within a group, in definition order).
pub fn all_codes() -> Vec<CodeInfo> {
    let mut out = Vec::new();
    for c in DiagCode::ALL {
        out.push(CodeInfo {
            group: CodeGroup::Diag,
            code: c.as_str(),
            args: c.arg_names(),
            severity: c.severity(),
        });
    }
    let plain = |group,
                 table: &'static [(&'static str, &'static [&'static str])],
                 out: &mut Vec<CodeInfo>| {
        for (code, args) in table {
            out.push(CodeInfo {
                group,
                code,
                args,
                severity: Severity::Error,
            });
        }
    };
    plain(CodeGroup::Vm, VM_CODES, &mut out);
    plain(CodeGroup::Load, LOAD_CODES, &mut out);
    plain(CodeGroup::TextError, TEXT_ERROR_CODES, &mut out);
    for (code, args, severity) in TEXT_ISSUE_CODES {
        out.push(CodeInfo {
            group: CodeGroup::TextIssue,
            code,
            args,
            severity: *severity,
        });
    }
    plain(CodeGroup::DeclError, DECL_ERROR_CODES, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decls::DeclErrorKind;
    use crate::registry::RegistryError;
    use crate::texts::{TextErrorKind, TextIssueKind};
    use crate::value::Ty;
    use crate::vm::{LoadErrorKind, VmErrorKind};

    fn s() -> String {
        String::new()
    }

    /// Indices of samples (one per kind). Adding a kind makes this fail to compile unless it is covered here.
    fn vm_index(k: &VmErrorKind) -> usize {
        match k {
            VmErrorKind::StepLimitExceeded => 0,
            VmErrorKind::Interrupted => 1,
            VmErrorKind::CallDepthExceeded => 2,
            VmErrorKind::ValueTooLarge => 3,
            VmErrorKind::MemoryLimitExceeded => 4,
            VmErrorKind::IndexOutOfRange { .. } => 5,
            VmErrorKind::KeyNotFound { .. } => 6,
            VmErrorKind::IntegerOverflow => 7,
            VmErrorKind::DivisionByZero => 8,
            VmErrorKind::NonFiniteFloat => 9,
            VmErrorKind::FloatOutOfRange => 10,
            VmErrorKind::NotStopped => 11,
            VmErrorKind::AlreadyStopped => 12,
            VmErrorKind::Ended => 13,
            VmErrorKind::NoChoiceAvailable => 14,
            VmErrorKind::ChoiceOutOfRange { .. } => 15,
            VmErrorKind::Poisoned => 16,
            VmErrorKind::PersistentData(_) => 17,
            VmErrorKind::InvalidRandomRange { .. } => 18,
            VmErrorKind::SnapshotMismatch => 19,
            VmErrorKind::BadInput { .. } => 20,
            VmErrorKind::StartLabelNotFound(_) => 21,
            VmErrorKind::CommandMissing(_) => 22,
            VmErrorKind::CommandFailed { .. } => 23,
            VmErrorKind::UnknownVariable(_) => 24,
            VmErrorKind::SliceSerialize(_) => 25,
            VmErrorKind::DeclOnlyRegistry => 26,
        }
    }

    #[test]
    fn runtime_error_list_matches_all_codes_in_order() {
        let samples = [
            VmErrorKind::StepLimitExceeded,
            VmErrorKind::Interrupted,
            VmErrorKind::CallDepthExceeded,
            VmErrorKind::ValueTooLarge,
            VmErrorKind::MemoryLimitExceeded,
            VmErrorKind::IndexOutOfRange { index: 0, len: 0 },
            VmErrorKind::KeyNotFound { key: s() },
            VmErrorKind::IntegerOverflow,
            VmErrorKind::DivisionByZero,
            VmErrorKind::NonFiniteFloat,
            VmErrorKind::FloatOutOfRange,
            VmErrorKind::NotStopped,
            VmErrorKind::AlreadyStopped,
            VmErrorKind::Ended,
            VmErrorKind::NoChoiceAvailable,
            VmErrorKind::ChoiceOutOfRange { index: 0, count: 0 },
            VmErrorKind::Poisoned,
            VmErrorKind::PersistentData(LoadErrorKind::Corrupt),
            VmErrorKind::InvalidRandomRange { min: 0, max: 0 },
            VmErrorKind::SnapshotMismatch,
            VmErrorKind::BadInput {
                expected: Ty::Int,
                found: Ty::Int,
            },
            VmErrorKind::StartLabelNotFound(s()),
            VmErrorKind::CommandMissing(s()),
            VmErrorKind::CommandFailed {
                code: s(),
                args: Vec::new(),
            },
            VmErrorKind::UnknownVariable(s()),
            VmErrorKind::SliceSerialize(s()),
            VmErrorKind::DeclOnlyRegistry,
        ];
        assert_eq!(samples.len(), VM_CODES.len());
        for (i, k) in samples.iter().enumerate() {
            assert_eq!(vm_index(k), i);
            assert_eq!(k.code(), VM_CODES[i].0);
        }
    }

    fn load_index(k: &LoadErrorKind) -> usize {
        match k {
            LoadErrorKind::BadMagic => 0,
            LoadErrorKind::UnsupportedVersion(_) => 1,
            LoadErrorKind::ChecksumMismatch => 2,
            LoadErrorKind::Corrupt => 3,
            LoadErrorKind::LabelNotFound(_) => 4,
            LoadErrorKind::ModuleMismatch(_) => 5,
            LoadErrorKind::CommandMissing(_) => 6,
            LoadErrorKind::StopArgsFailed => 7,
            LoadErrorKind::PackageSlice(_) => 8,
            LoadErrorKind::DeclOnlyRegistry => 9,
            LoadErrorKind::SaveFromNewerVersion { .. } => 10,
            LoadErrorKind::SaveVersionUnsupported { .. } => 11,
            LoadErrorKind::SchemaMismatch(_) => 12,
            LoadErrorKind::MigrateFailed(_) => 13,
        }
    }

    #[test]
    fn load_error_list_matches_all_codes_in_order() {
        let samples = [
            LoadErrorKind::BadMagic,
            LoadErrorKind::UnsupportedVersion(0),
            LoadErrorKind::ChecksumMismatch,
            LoadErrorKind::Corrupt,
            LoadErrorKind::LabelNotFound(s()),
            LoadErrorKind::ModuleMismatch(s()),
            LoadErrorKind::CommandMissing(s()),
            LoadErrorKind::StopArgsFailed,
            LoadErrorKind::PackageSlice(s()),
            LoadErrorKind::DeclOnlyRegistry,
            LoadErrorKind::SaveFromNewerVersion {
                saved: 0,
                current: 0,
            },
            LoadErrorKind::SaveVersionUnsupported { saved: 0, min: 0 },
            LoadErrorKind::SchemaMismatch(Vec::new()),
            LoadErrorKind::MigrateFailed(Vec::new()),
        ];
        assert_eq!(samples.len(), LOAD_CODES.len());
        for (i, k) in samples.iter().enumerate() {
            assert_eq!(load_index(k), i);
            assert_eq!(k.code(), LOAD_CODES[i].0);
        }
    }

    fn text_error_index(k: &TextErrorKind) -> usize {
        match k {
            TextErrorKind::InvalidEscape { .. } => 0,
            TextErrorKind::InvalidPlaceholder { .. } => 1,
            TextErrorKind::MissingTab => 2,
            TextErrorKind::EmptyKey => 3,
            TextErrorKind::DuplicateKey => 4,
            TextErrorKind::UnknownDirective { .. } => 5,
        }
    }

    #[test]
    fn text_table_error_list_matches_all_codes_in_order() {
        let samples = [
            TextErrorKind::InvalidEscape { found: s() },
            TextErrorKind::InvalidPlaceholder { found: s() },
            TextErrorKind::MissingTab,
            TextErrorKind::EmptyKey,
            TextErrorKind::DuplicateKey,
            TextErrorKind::UnknownDirective { name: s() },
        ];
        assert_eq!(samples.len(), TEXT_ERROR_CODES.len());
        for (i, k) in samples.iter().enumerate() {
            assert_eq!(text_error_index(k), i);
            assert_eq!(k.code(), TEXT_ERROR_CODES[i].0);
        }
    }

    fn text_issue_index(k: &TextIssueKind) -> usize {
        match k {
            TextIssueKind::Missing => 0,
            TextIssueKind::Unused => 1,
            TextIssueKind::BadNumber { .. } => 2,
        }
    }

    #[test]
    fn match_result_list_has_same_codes_and_severities_for_all_kinds() {
        let samples = [
            TextIssueKind::Missing,
            TextIssueKind::Unused,
            TextIssueKind::BadNumber {
                number: 1,
                count: 0,
            },
        ];
        assert_eq!(samples.len(), TEXT_ISSUE_CODES.len());
        for (i, k) in samples.iter().enumerate() {
            assert_eq!(text_issue_index(k), i);
            assert_eq!(k.code(), TEXT_ISSUE_CODES[i].0);
            assert_eq!(k.severity(), TEXT_ISSUE_CODES[i].2);
        }
    }

    fn decl_index(k: &DeclErrorKind) -> usize {
        match k {
            DeclErrorKind::Syntax { .. } => 0,
            DeclErrorKind::TooLarge => 1,
            DeclErrorKind::TooDeep => 2,
            DeclErrorKind::NotDecls => 3,
            DeclErrorKind::UnsupportedVersion(_) => 4,
            DeclErrorKind::MissingField => 5,
            DeclErrorKind::WrongType => 6,
            DeclErrorKind::InvalidType(_) => 7,
            DeclErrorKind::InvalidValue => 8,
            DeclErrorKind::Registry(_) => 9,
        }
    }

    #[test]
    fn declaration_file_error_list_matches_all_codes_in_order() {
        let samples = [
            DeclErrorKind::Syntax { line: 1, col: 1 },
            DeclErrorKind::TooLarge,
            DeclErrorKind::TooDeep,
            DeclErrorKind::NotDecls,
            DeclErrorKind::UnsupportedVersion(0),
            DeclErrorKind::MissingField,
            DeclErrorKind::WrongType,
            DeclErrorKind::InvalidType(s()),
            DeclErrorKind::InvalidValue,
            DeclErrorKind::Registry(RegistryError::NoCommand),
        ];
        assert_eq!(samples.len(), DECL_ERROR_CODES.len());
        for (i, k) in samples.iter().enumerate() {
            assert_eq!(decl_index(k), i);
            assert_eq!(k.code(), DECL_ERROR_CODES[i].0);
        }
    }

    #[test]
    fn diagnostic_codes_are_unique_and_names_are_lowercase_and_underscores() {
        let all = all_codes();
        for g in CodeGroup::ALL {
            let mut seen = std::collections::HashSet::new();
            for c in all.iter().filter(|c| c.group == *g) {
                assert!(seen.insert(c.code), "{}: {} が重複", g.name(), c.code);
                assert!(
                    c.code
                        .chars()
                        .all(|ch| ch.is_ascii_lowercase() || ch == '_'),
                    "{}",
                    c.code
                );
            }
        }
        assert_eq!(
            all.iter().filter(|c| c.group == CodeGroup::Diag).count(),
            DiagCode::ALL.len()
        );
    }
}
