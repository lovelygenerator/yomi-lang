//! Diagnostics (errors and warnings). Only codes and arguments; no message text.

/// Severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The script cannot run.
    Error,
    /// It can run, but there may be a mistake.
    Warning,
}

/// A diagnostic code. `as_str` is the stable English name.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagCode {
    /// An unexpected token. Argument `found`.
    UnexpectedToken,
    /// A string literal is not closed.
    UnterminatedString,
    /// An escape that cannot be used. Argument `found`.
    InvalidEscape,
    /// Cannot be read as a number. Argument `text`.
    InvalidNumber,
    /// There is a full-width space in code.
    FullWidthSpace,
    /// An undeclared name. Argument `name`, and `suggestions` if there are close names (up to 3, closest first).
    UnknownName,
    /// An unregistered command. Argument `name`, and `suggestions` if there are close names.
    UnknownCommand,
    /// A label that does not exist. Argument `name`, and `suggestions` if there are close names.
    UnknownLabel,
    /// An unknown type name. Argument `name`.
    UnknownType,
    /// Types do not match. Arguments `expected`, `found`.
    TypeMismatch,
    /// A condition is not `bool`. Argument `found`.
    ConditionNotBool,
    /// The number of arguments does not match. Arguments `name`, `expected`, `found`.
    ArgCount,
    /// There is no parameter with that name. Arguments `name` (what was called), `arg` (the parameter name).
    UnknownArgument,
    /// The same argument was passed twice. Arguments `name`, `arg`.
    DuplicateArgument,
    /// A parameter without a default value was not passed. Arguments `name`, `arg`.
    MissingArgument,
    /// Named arguments are not in declaration order. Arguments `name`, `arg`.
    ArgumentOutOfOrder,
    /// A positional argument was written after a named argument. Argument `name`.
    PositionalAfterNamed,
    /// A parameter without a default value was declared after one with a default value. Arguments `name`, `arg`.
    RequiredAfterDefault,
    /// Names collide. Argument `name`.
    DuplicateName,
    /// A stable ID collides within the same label. Argument `id`.
    DuplicateStableId,
    /// The form of a stable ID is invalid. Argument `id`.
    InvalidStableId,
    /// A sugar is not bound. Argument `kind`.
    UnboundSugar,
    /// A block is not closed.
    UnterminatedBlock,
    /// A `}` without a matching opening.
    UnexpectedClose,
    /// An `else` without an `if`.
    ElseWithoutIf,
    /// There is a statement or text outside a label.
    StatementOutsideLabel,
    /// An initial value that is not a constant.
    NotConstant,
    /// Syntax that is not implemented yet (but is in the spec). Argument `what` (a call of a name that cannot be called,
    /// `call`; a postfix call, `postfix expression`; the name of a return type that cannot be stored in a variable; and so on).
    NotYetSupported,
    /// Expressions are nested or chained too deeply. Argument `limit`.
    TooDeep,
    /// A line ends where more was expected.
    UnexpectedEol,
    /// Blocks are nested too deeply. Argument `limit`.
    BlockTooDeep,
    /// One label or `fn` uses too many registers (local variables and temporaries) (the register backend). Argument `limit`.
    TooManyRegisters,
    /// An operation that cannot be used on this type. Arguments `op` (the operator, such as `+`, `<`, `-`, `+=`), `type`
    /// (the type of the operand).
    InvalidOperation,
    /// A declaration that cannot be written inside a label (such as `save var`). Argument `what` (the first word).
    DeclarationNotAllowed,
    /// `break` / `continue` outside a loop. Argument `what` (`break` or `continue`).
    OutsideLoop,
    /// A variable was declared with the same name as a built-in function (warning). Argument `name`.
    ShadowedBuiltin,
    /// An `import` is written after other declarations.
    ImportNotFirst,
    /// A module that does not exist. Argument `name`, and `suggestions` if there are close names (up to 3, closest first).
    UnknownModule,
    /// A module that is not `import`ed was used. Argument `name`.
    ModuleNotImported,
    /// Module names collide. Argument `name`.
    DuplicateModule,
    /// A module name is not a valid identifier. Argument `name`.
    InvalidModuleName,
    /// An alias was given with `as` to the `import` of a package. Argument `name` (the package name).
    PackageAlias,
    /// A name used by an `import` collides with a name already `import`ed (an alias can be given with `as`). Argument `name`.
    DuplicateImport,
    /// `import`ed but not used (warning). Argument `name` (the path written in the `import`).
    UnusedImport,
    /// A local variable declared with `let` is never read (warning). Argument `name`.
    UnusedVariable,
    /// A `jump` to a label that has parameters or a return type (it can only be called with `call`). Argument `name`.
    JumpToCallableLabel,
    /// A `jump` inside a label that has a return type (it could no longer return a value to the caller). Argument `name`
    /// (the name of that label).
    JumpInReturningLabel,
    /// A `call` in the middle of an expression (it can only be written on the right-hand side of `let` or `=`, or as a
    /// statement).
    CallInExpression,
    /// Tried to receive the value of a label that has no return type. Argument `name`.
    CallWithoutValue,
    /// A label with a return type may reach its end without `return`. Argument `name`.
    MissingReturn,
    /// A `return` without a value in a label with a return type. Argument `expected` (the type name).
    MissingReturnValue,
    /// A `return` with a value in a label without a return type.
    UnexpectedReturnValue,
    /// A label that is never `call`ed and does not end with `jump` / `return` / `end` (warning). Argument `name`.
    LabelFallsThrough,
    /// The `「` of a quote line or an option is not closed by `」` (quote lines with an identity, and option texts).
    UnterminatedQuote,
    /// A bracket of an expression is not closed by the end of the file or the limit of continuation lines. Argument `open`.
    UnclosedBracket,
    /// In a quote line with an identity, characters other than spaces and a stable ID follow `」`.
    TextAfterQuote,
    /// An option with `once` has no stable ID.
    OnceWithoutId,
    /// A `choice` with no options.
    EmptyChoice,
    /// A versioned name (`v2::…`) written outside the `migrations` module. Argument `name`.
    SchemaNameOutsideMigrations,
    /// A frozen schema file with a wrong file name or header. Argument `path`.
    InvalidSchemaFile,
    /// The content of a frozen schema does not match its recorded hash. Argument `version`.
    SchemaHashMismatch,
    /// Two `migrate` blocks of the same version. Argument `version`.
    DuplicateMigrate,
    /// A `migrate` version below 2. Argument `version`.
    InvalidMigrateVersion,
    /// A wrong `min_save_version` (out of range, repeated, not first). Argument `version`.
    InvalidMinSaveVersion,
    /// A name not in the schema of that version. Arguments `name`, `version`, and `suggestions` if there are close names.
    UnknownSchemaName,
    /// A `rename` between a `save var` and a `persistent var`. Argument `name`.
    MigrateKindMismatch,
    /// A written type differs from the schema's type. Arguments `name`, `expected`, `found`.
    MigrateTypeMismatch,
    /// The same variable handled twice in one `migrate`. Argument `name`.
    DuplicateMigrateTarget,
    /// Something not allowed in a conversion expression. Argument `what`.
    MigrateImpureExpr,
    /// A needed frozen schema is missing. Argument `version`.
    MissingFrozenSchema,
    /// A needed `migrate` block is missing. Argument `version`.
    MissingMigrate,
    /// A variable whose type changed is not handled by `migrate` (warning). Arguments `name`, `version`.
    UnhandledTypeChange,
    /// The schema changed after the latest freeze but there is no newer `migrate` (warning). Argument `version`.
    SchemaChangedSinceFreeze,
    /// An identity with the same name exists in multiple `import`ed modules. Arguments `name`, `candidates` (a list of
    /// `module.name`).
    AmbiguousIdentity,
    /// A plain line where an undeclared name is followed by `「`, and there is an identity with a close spelling (warning).
    /// Arguments `name`, `suggestions`.
    IdentityTypo,
    /// The string literal passed to `seen` is not the position of an existing stop point (warning). Argument `position`.
    UnknownPosition,
    /// The string literal passed to `visited` is not an existing label (warning). Argument `label`.
    UnknownLabelLiteral,
    /// A use that does not fit the kind of command (an effect or stop command called inside an expression, a query command
    /// called as an `@` statement, an `@` call in the middle of an expression, or an `@` call of a command returning `()` or of
    /// a query command on the right-hand side of `let` or `=`).
    /// Arguments `name` (the command name), `kind` (`effect`, `query`, `stop`).
    InvalidCommandUse,
    /// Assignment to a `read_only` variable. Argument `name` (`package.variable`).
    AssignToReadOnly,
    /// A `match` does not cover all values. Argument `missing` (a list of missing patterns, such as `None`, `Dir.South`,
    /// `_`).
    NonExhaustiveMatch,
    /// An arm of a `match` is hidden by an earlier arm and is never chosen (warning).
    UnreachablePattern,
    /// A field the type does not have. Arguments `name` (the field name), `type` (the type name), and `suggestions` if there
    /// are close names.
    UnknownField,
    /// A variant the type does not have. Arguments `name` (the variant name), `type` (the type name), and `suggestions` if
    /// there are close names.
    UnknownVariant,
    /// A struct literal is missing a field. Arguments `name` (the field name), `type` (the type name).
    MissingField,
    /// The same field is written twice (in a struct declaration or literal). Argument `name`.
    DuplicateField,
    /// A type that contains itself (directly or through other types). Argument `name` (the type name).
    RecursiveType,
    /// An expression whose type cannot be decided (`[]` or `None` without a type annotation). Add a type annotation
    /// (`let xs: list<int> = []`).
    CannotInferType,
    /// A method the type does not have. Arguments `name` (the method name), `type` (the name of the receiver's type).
    UnknownMethod,
    /// A method that updates a variable (`push`, `remove`, `clear`) was called inside an expression. Write it as a statement.
    /// Argument `name`.
    StatementMethodInExpression,
    /// A `list` method that needs ordered or summable elements (`sort`, `sort_by`, `min_by`, `max_by`, `sum`) was used with a
    /// type that is not `int`, `float`, `str` or `bool` (`sum`: `int` or `float`). Arguments `method` and `type` (the type of
    /// the elements, or of the keys for `sort_by`, `min_by` and `max_by`).
    UnsupportedElementType,
    /// A type that cannot be a `map` key (`float`, `list`, `map`, `Option`, and `struct`s and `enum`s containing them).
    /// Argument `type` (the name of the type written as the key).
    InvalidMapKey,
    /// A statement that cannot be written inside a `fn` (quote lines, plain lines, `choice`, `jump`, `call`, `end`, and
    /// effect and stop commands). Argument `what` (`plain`, `quote`, `choice`, `jump`, `call`, `end`; for commands, the name
    /// with `@`, such as `@audio.play`).
    NotAllowedInFn,
    /// Tried to write to a `save var`, a `persistent var` or a host-declared variable inside a `fn` (reading is fine).
    /// Argument `name` (the name of the written variable; variables of other modules are `module.name`).
    AssignInFn,
    /// An undeclared `fn` (a call `name(...)`, `module.name(...)` or `@name(...)`). Argument `name`, and `suggestions` if
    /// there are close names (up to 3, closest first).
    UnknownFunction,
    /// A text line (plain line, quote line or option text) without a stable ID while `CompileOptions::require_text_ids` is
    /// enabled. Argument `kind` (`plain`, `quote`, `choice`).
    MissingTextId,
    /// A command was called by an old name (alias) (warning). Arguments `name` (the old name written), `new` (the current
    /// name).
    DeprecatedCommandName,
    /// Tried to put a value containing a function type in a place that goes into saves. Arguments `type` (the type name),
    /// `what` (`var`: `save var` or `persistent var`; `field`: a field or argument of a `struct` or `enum`; `stop`: a local
    /// variable that lives across a stop point; `call`: a local variable that lives across a `call`; `repl`: a REPL `let`).
    FnNotSaveable,
    /// Something that cannot be written in REPL input (section 7.7 of `spec/host/07-repl.md`). Argument `what` (`stable_id`,
    /// `once`, `save`, `persistent`, `label`, `jump`, `end`, `return` (outside a `fn`)).
    ReplNotAllowed,
}

impl DiagCode {
    /// All codes (used to check that translation keys are complete).
    pub const ALL: &'static [DiagCode] = &[
        DiagCode::UnexpectedToken,
        DiagCode::UnterminatedString,
        DiagCode::InvalidEscape,
        DiagCode::InvalidNumber,
        DiagCode::FullWidthSpace,
        DiagCode::UnknownName,
        DiagCode::UnknownCommand,
        DiagCode::UnknownLabel,
        DiagCode::UnknownType,
        DiagCode::TypeMismatch,
        DiagCode::ConditionNotBool,
        DiagCode::ArgCount,
        DiagCode::UnknownArgument,
        DiagCode::DuplicateArgument,
        DiagCode::MissingArgument,
        DiagCode::ArgumentOutOfOrder,
        DiagCode::PositionalAfterNamed,
        DiagCode::RequiredAfterDefault,
        DiagCode::DuplicateName,
        DiagCode::DuplicateStableId,
        DiagCode::InvalidStableId,
        DiagCode::UnboundSugar,
        DiagCode::UnterminatedBlock,
        DiagCode::UnexpectedClose,
        DiagCode::ElseWithoutIf,
        DiagCode::StatementOutsideLabel,
        DiagCode::NotConstant,
        DiagCode::NotYetSupported,
        DiagCode::TooDeep,
        DiagCode::UnexpectedEol,
        DiagCode::BlockTooDeep,
        DiagCode::TooManyRegisters,
        DiagCode::InvalidOperation,
        DiagCode::DeclarationNotAllowed,
        DiagCode::OutsideLoop,
        DiagCode::ShadowedBuiltin,
        DiagCode::ImportNotFirst,
        DiagCode::UnknownModule,
        DiagCode::ModuleNotImported,
        DiagCode::DuplicateModule,
        DiagCode::InvalidModuleName,
        DiagCode::PackageAlias,
        DiagCode::DuplicateImport,
        DiagCode::UnusedImport,
        DiagCode::UnusedVariable,
        DiagCode::JumpToCallableLabel,
        DiagCode::JumpInReturningLabel,
        DiagCode::CallInExpression,
        DiagCode::CallWithoutValue,
        DiagCode::MissingReturn,
        DiagCode::MissingReturnValue,
        DiagCode::UnexpectedReturnValue,
        DiagCode::LabelFallsThrough,
        DiagCode::UnterminatedQuote,
        DiagCode::UnclosedBracket,
        DiagCode::TextAfterQuote,
        DiagCode::OnceWithoutId,
        DiagCode::EmptyChoice,
        DiagCode::SchemaNameOutsideMigrations,
        DiagCode::InvalidSchemaFile,
        DiagCode::SchemaHashMismatch,
        DiagCode::DuplicateMigrate,
        DiagCode::InvalidMigrateVersion,
        DiagCode::InvalidMinSaveVersion,
        DiagCode::UnknownSchemaName,
        DiagCode::MigrateKindMismatch,
        DiagCode::MigrateTypeMismatch,
        DiagCode::DuplicateMigrateTarget,
        DiagCode::MigrateImpureExpr,
        DiagCode::MissingFrozenSchema,
        DiagCode::MissingMigrate,
        DiagCode::UnhandledTypeChange,
        DiagCode::SchemaChangedSinceFreeze,
        DiagCode::AmbiguousIdentity,
        DiagCode::IdentityTypo,
        DiagCode::UnknownPosition,
        DiagCode::UnknownLabelLiteral,
        DiagCode::InvalidCommandUse,
        DiagCode::AssignToReadOnly,
        DiagCode::NonExhaustiveMatch,
        DiagCode::UnreachablePattern,
        DiagCode::UnknownField,
        DiagCode::UnknownVariant,
        DiagCode::MissingField,
        DiagCode::DuplicateField,
        DiagCode::RecursiveType,
        DiagCode::CannotInferType,
        DiagCode::UnknownMethod,
        DiagCode::StatementMethodInExpression,
        DiagCode::UnsupportedElementType,
        DiagCode::InvalidMapKey,
        DiagCode::NotAllowedInFn,
        DiagCode::AssignInFn,
        DiagCode::UnknownFunction,
        DiagCode::MissingTextId,
        DiagCode::DeprecatedCommandName,
        DiagCode::ReplNotAllowed,
        DiagCode::FnNotSaveable,
    ];

    /// The severity of this code (codes other than warnings are errors).
    pub fn severity(self) -> Severity {
        match self {
            DiagCode::IdentityTypo
            | DiagCode::UnknownPosition
            | DiagCode::UnknownLabelLiteral
            | DiagCode::UnreachablePattern
            | DiagCode::ShadowedBuiltin
            | DiagCode::LabelFallsThrough
            | DiagCode::UnusedImport
            | DiagCode::UnusedVariable
            | DiagCode::UnhandledTypeChange
            | DiagCode::SchemaChangedSinceFreeze
            | DiagCode::DeprecatedCommandName => Severity::Warning,
            _ => Severity::Error,
        }
    }

    /// The argument names that diagnostics with this code have (the public specification of diagnostics; `suggestions` is
    /// present only when there are close names).
    pub fn arg_names(self) -> &'static [&'static str] {
        match self {
            DiagCode::UnexpectedToken | DiagCode::InvalidEscape => &["found"],
            DiagCode::InvalidNumber => &["text"],
            DiagCode::UnknownName
            | DiagCode::UnknownFunction
            | DiagCode::UnknownCommand
            | DiagCode::UnknownLabel
            | DiagCode::UnknownModule => &["name", "suggestions"],
            DiagCode::UnknownField | DiagCode::UnknownVariant | DiagCode::UnknownMethod => {
                &["name", "type", "suggestions"]
            }
            DiagCode::IdentityTypo => &["name", "suggestions"],
            DiagCode::UnknownType
            | DiagCode::DuplicateName
            | DiagCode::DuplicateField
            | DiagCode::RecursiveType
            | DiagCode::StatementMethodInExpression
            | DiagCode::ModuleNotImported
            | DiagCode::DuplicateModule
            | DiagCode::InvalidModuleName
            | DiagCode::PackageAlias
            | DiagCode::DuplicateImport
            | DiagCode::UnusedImport
            | DiagCode::UnusedVariable
            | DiagCode::JumpToCallableLabel
            | DiagCode::JumpInReturningLabel
            | DiagCode::CallWithoutValue
            | DiagCode::MissingReturn
            | DiagCode::AssignToReadOnly
            | DiagCode::AssignInFn
            | DiagCode::ShadowedBuiltin
            | DiagCode::LabelFallsThrough => &["name"],
            DiagCode::SchemaNameOutsideMigrations
            | DiagCode::MigrateKindMismatch
            | DiagCode::DuplicateMigrateTarget => &["name"],
            DiagCode::InvalidSchemaFile => &["path"],
            DiagCode::SchemaHashMismatch
            | DiagCode::DuplicateMigrate
            | DiagCode::InvalidMigrateVersion
            | DiagCode::InvalidMinSaveVersion
            | DiagCode::MissingFrozenSchema
            | DiagCode::MissingMigrate
            | DiagCode::SchemaChangedSinceFreeze => &["version"],
            DiagCode::UnknownSchemaName => &["name", "version", "suggestions"],
            DiagCode::MigrateTypeMismatch => &["name", "expected", "found"],
            DiagCode::MigrateImpureExpr => &["what"],
            DiagCode::UnclosedBracket => &["open"],
            DiagCode::UnhandledTypeChange => &["name", "version"],
            DiagCode::MissingField => &["name", "type"],
            DiagCode::UnsupportedElementType => &["method", "type"],
            DiagCode::NonExhaustiveMatch => &["missing"],
            DiagCode::TypeMismatch => &["expected", "found"],
            DiagCode::ConditionNotBool => &["found"],
            DiagCode::ArgCount => &["name", "expected", "found"],
            DiagCode::UnknownArgument
            | DiagCode::DuplicateArgument
            | DiagCode::MissingArgument
            | DiagCode::ArgumentOutOfOrder
            | DiagCode::RequiredAfterDefault => &["name", "arg"],
            DiagCode::PositionalAfterNamed => &["name"],
            DiagCode::InvalidOperation => &["op", "type"],
            DiagCode::DuplicateStableId | DiagCode::InvalidStableId => &["id"],
            DiagCode::UnboundSugar | DiagCode::MissingTextId => &["kind"],
            DiagCode::InvalidCommandUse => &["name", "kind"],
            DiagCode::DeclarationNotAllowed
            | DiagCode::OutsideLoop
            | DiagCode::NotAllowedInFn
            | DiagCode::ReplNotAllowed
            | DiagCode::NotYetSupported => &["what"],
            DiagCode::InvalidMapKey => &["type"],
            DiagCode::FnNotSaveable => &["type", "what"],
            DiagCode::TooDeep | DiagCode::BlockTooDeep | DiagCode::TooManyRegisters => &["limit"],
            DiagCode::MissingReturnValue => &["expected"],
            DiagCode::AmbiguousIdentity => &["name", "candidates"],
            DiagCode::UnknownPosition => &["position"],
            DiagCode::UnknownLabelLiteral => &["label"],
            DiagCode::DeprecatedCommandName => &["name", "new"],
            DiagCode::UnterminatedString
            | DiagCode::FullWidthSpace
            | DiagCode::UnterminatedBlock
            | DiagCode::UnexpectedClose
            | DiagCode::ElseWithoutIf
            | DiagCode::StatementOutsideLabel
            | DiagCode::NotConstant
            | DiagCode::UnexpectedEol
            | DiagCode::ImportNotFirst
            | DiagCode::CallInExpression
            | DiagCode::UnexpectedReturnValue
            | DiagCode::UnterminatedQuote
            | DiagCode::TextAfterQuote
            | DiagCode::OnceWithoutId
            | DiagCode::EmptyChoice
            | DiagCode::UnreachablePattern
            | DiagCode::CannotInferType => &[],
        }
    }

    /// The stable English name.
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::UnexpectedToken => "unexpected_token",
            DiagCode::UnterminatedString => "unterminated_string",
            DiagCode::InvalidEscape => "invalid_escape",
            DiagCode::InvalidNumber => "invalid_number",
            DiagCode::FullWidthSpace => "full_width_space",
            DiagCode::UnknownName => "unknown_name",
            DiagCode::UnknownCommand => "unknown_command",
            DiagCode::UnknownLabel => "unknown_label",
            DiagCode::UnknownType => "unknown_type",
            DiagCode::TypeMismatch => "type_mismatch",
            DiagCode::ConditionNotBool => "condition_not_bool",
            DiagCode::ArgCount => "arg_count",
            DiagCode::UnknownArgument => "unknown_argument",
            DiagCode::DuplicateArgument => "duplicate_argument",
            DiagCode::MissingArgument => "missing_argument",
            DiagCode::ArgumentOutOfOrder => "argument_out_of_order",
            DiagCode::PositionalAfterNamed => "positional_after_named",
            DiagCode::RequiredAfterDefault => "required_after_default",
            DiagCode::DuplicateName => "duplicate_name",
            DiagCode::DuplicateStableId => "duplicate_stable_id",
            DiagCode::InvalidStableId => "invalid_stable_id",
            DiagCode::UnboundSugar => "unbound_sugar",
            DiagCode::UnterminatedBlock => "unterminated_block",
            DiagCode::UnexpectedClose => "unexpected_close",
            DiagCode::ElseWithoutIf => "else_without_if",
            DiagCode::StatementOutsideLabel => "statement_outside_label",
            DiagCode::NotConstant => "not_constant",
            DiagCode::NotYetSupported => "not_yet_supported",
            DiagCode::TooDeep => "too_deep",
            DiagCode::UnexpectedEol => "unexpected_eol",
            DiagCode::BlockTooDeep => "block_too_deep",
            DiagCode::TooManyRegisters => "too_many_registers",
            DiagCode::InvalidOperation => "invalid_operation",
            DiagCode::DeclarationNotAllowed => "declaration_not_allowed",
            DiagCode::OutsideLoop => "outside_loop",
            DiagCode::ShadowedBuiltin => "shadowed_builtin",
            DiagCode::ImportNotFirst => "import_not_first",
            DiagCode::UnknownModule => "unknown_module",
            DiagCode::ModuleNotImported => "module_not_imported",
            DiagCode::DuplicateModule => "duplicate_module",
            DiagCode::InvalidModuleName => "invalid_module_name",
            DiagCode::PackageAlias => "package_alias",
            DiagCode::DuplicateImport => "duplicate_import",
            DiagCode::UnusedImport => "unused_import",
            DiagCode::UnusedVariable => "unused_variable",
            DiagCode::JumpToCallableLabel => "jump_to_callable_label",
            DiagCode::JumpInReturningLabel => "jump_in_returning_label",
            DiagCode::CallInExpression => "call_in_expression",
            DiagCode::CallWithoutValue => "call_without_value",
            DiagCode::MissingReturn => "missing_return",
            DiagCode::MissingReturnValue => "missing_return_value",
            DiagCode::UnexpectedReturnValue => "unexpected_return_value",
            DiagCode::LabelFallsThrough => "label_falls_through",
            DiagCode::UnterminatedQuote => "unterminated_quote",
            DiagCode::UnclosedBracket => "unclosed_bracket",
            DiagCode::TextAfterQuote => "text_after_quote",
            DiagCode::OnceWithoutId => "once_without_id",
            DiagCode::SchemaNameOutsideMigrations => "schema_name_outside_migrations",
            DiagCode::InvalidSchemaFile => "invalid_schema_file",
            DiagCode::SchemaHashMismatch => "schema_hash_mismatch",
            DiagCode::DuplicateMigrate => "duplicate_migrate",
            DiagCode::InvalidMigrateVersion => "invalid_migrate_version",
            DiagCode::InvalidMinSaveVersion => "invalid_min_save_version",
            DiagCode::UnknownSchemaName => "unknown_schema_name",
            DiagCode::MigrateKindMismatch => "migrate_kind_mismatch",
            DiagCode::MigrateTypeMismatch => "migrate_type_mismatch",
            DiagCode::DuplicateMigrateTarget => "duplicate_migrate_target",
            DiagCode::MigrateImpureExpr => "migrate_impure_expr",
            DiagCode::MissingFrozenSchema => "missing_frozen_schema",
            DiagCode::MissingMigrate => "missing_migrate",
            DiagCode::UnhandledTypeChange => "unhandled_type_change",
            DiagCode::SchemaChangedSinceFreeze => "schema_changed_since_freeze",
            DiagCode::EmptyChoice => "empty_choice",
            DiagCode::AmbiguousIdentity => "ambiguous_identity",
            DiagCode::IdentityTypo => "identity_typo",
            DiagCode::UnknownPosition => "unknown_position",
            DiagCode::UnknownLabelLiteral => "unknown_label_literal",
            DiagCode::InvalidCommandUse => "invalid_command_use",
            DiagCode::AssignToReadOnly => "assign_to_read_only",
            DiagCode::NonExhaustiveMatch => "non_exhaustive_match",
            DiagCode::UnreachablePattern => "unreachable_pattern",
            DiagCode::UnknownField => "unknown_field",
            DiagCode::UnknownVariant => "unknown_variant",
            DiagCode::MissingField => "missing_field",
            DiagCode::DuplicateField => "duplicate_field",
            DiagCode::RecursiveType => "recursive_type",
            DiagCode::CannotInferType => "cannot_infer_type",
            DiagCode::UnknownMethod => "unknown_method",
            DiagCode::StatementMethodInExpression => "statement_method_in_expression",
            DiagCode::UnsupportedElementType => "unsupported_element_type",
            DiagCode::InvalidMapKey => "invalid_map_key",
            DiagCode::NotAllowedInFn => "not_allowed_in_fn",
            DiagCode::AssignInFn => "assign_in_fn",
            DiagCode::UnknownFunction => "unknown_function",
            DiagCode::MissingTextId => "missing_text_id",
            DiagCode::DeprecatedCommandName => "deprecated_command_name",
            DiagCode::ReplNotAllowed => "repl_not_allowed",
            DiagCode::FnNotSaveable => "fn_not_saveable",
        }
    }
}

/// The value of a diagnostic argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagArg {
    /// A string.
    Str(String),
    /// An integer.
    Int(i64),
    /// A list of strings (such as candidate close names, closest first).
    List(Vec<String>),
}

impl From<&str> for DiagArg {
    fn from(s: &str) -> Self {
        DiagArg::Str(s.to_string())
    }
}

impl From<String> for DiagArg {
    fn from(s: String) -> Self {
        DiagArg::Str(s)
    }
}

impl From<i64> for DiagArg {
    fn from(n: i64) -> Self {
        DiagArg::Int(n)
    }
}

impl From<usize> for DiagArg {
    /// Values that do not fit in `i64` become `i64::MAX`.
    fn from(n: usize) -> Self {
        DiagArg::Int(i64::try_from(n).unwrap_or(i64::MAX))
    }
}

/// Writes only the value in a human-readable form (strings as is, integers in decimal, lists separated by `,`). Not a
/// message.
impl std::fmt::Display for DiagArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagArg::Str(s) => write!(f, "{s}"),
            DiagArg::Int(n) => write!(f, "{n}"),
            DiagArg::List(l) => write!(f, "{}", l.join(",")),
        }
    }
}

/// One diagnostic.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Module name.
    pub module: String,
    /// Line (1-based).
    pub line: u32,
    /// Column (1-based, in characters).
    pub col: u32,
    /// Severity.
    pub severity: Severity,
    /// Code.
    pub code: DiagCode,
    /// Named arguments determined by each code.
    pub args: Vec<(&'static str, DiagArg)>,
}

impl Diagnostic {
    /// Looks up an argument that is a list of strings.
    pub fn arg_list(&self, name: &str) -> Option<&[String]> {
        self.args.iter().find_map(|(n, v)| match v {
            DiagArg::List(l) if *n == name => Some(l.as_slice()),
            _ => None,
        })
    }

    /// Looks up a string argument.
    pub fn arg_str(&self, name: &str) -> Option<&str> {
        self.args.iter().find_map(|(n, v)| match v {
            DiagArg::Str(s) if *n == name => Some(s.as_str()),
            _ => None,
        })
    }
}
