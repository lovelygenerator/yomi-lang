//! Internal types of the compiler (built-in functions, signatures, tables of module declarations, compiler state).

use super::*;

/// The type of an expression (`None` if it cannot be decided; a diagnostic was emitted) and the IR expression.
pub(super) type Typed = (Option<Ty>, ExprId);

/// One row of a type's method table (name, parameter types, instruction, return type).
pub(super) type MethodSig<'a> = (&'a str, Vec<Ty>, Method, Ty);

/// The names of built-in functions in the spec (section 3.8 of `spec/language/03-statements.md`). Declaring a variable
/// with the same name gives a warning.
pub(super) const BUILTIN_NAMES: &[&str] = &[
    "random",
    "random_float",
    "seen",
    "visited",
    "to_int",
    "to_float",
    "range",
];

impl Builtin {
    /// Looks up the kind from the name of a built-in function. `None` if it is not a built-in function.
    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            "to_int" => Some(Builtin::ToInt),
            "to_float" => Some(Builtin::ToFloat),
            "seen" => Some(Builtin::Seen),
            "visited" => Some(Builtin::Visited),
            "random" => Some(Builtin::Random),
            "random_float" => Some(Builtin::RandomFloat),
            "range" => Some(Builtin::Range),
            _ => None,
        }
    }

    /// Parameter types.
    pub(super) fn params(self) -> &'static [Ty] {
        match self {
            Builtin::ToInt => &[Ty::Float],
            Builtin::ToFloat => &[Ty::Int],
            Builtin::Seen | Builtin::Visited => &[Ty::Str],
            Builtin::Random | Builtin::Range => &[Ty::Int, Ty::Int],
            Builtin::RandomFloat => &[],
        }
    }

    /// The return type of a built-in function.
    pub(super) fn ret(self) -> Ty {
        match self {
            Builtin::Range => Ty::List(Box::new(Ty::Int)),
            Builtin::ToInt | Builtin::Random => Ty::Int,
            Builtin::ToFloat | Builtin::RandomFloat => Ty::Float,
            Builtin::Seen | Builtin::Visited => Ty::Bool,
        }
    }
}

/// What the first half of `name.` refers to.
pub(super) enum Target {
    /// A module (index).
    Module(usize),
    /// An `import`ed package.
    Package,
}

/// References within a label recorded by the front end (used for diagnostics; independent of the shape of the code).
#[derive(Debug, Default)]
pub(super) struct LabelRefs {
    /// The stable ID of a stop point (including stable IDs of options).
    pub(super) stop_ids: Vec<String>,
    /// The target label of `call` / `jump` (a sequential number across the whole program).
    pub(super) targets: Vec<u32>,
}

/// The return type of a label.
#[derive(Debug, Clone, Default)]
pub(super) enum RetTy {
    /// There is no return type.
    #[default]
    None,
    Ty(Ty),
    /// The type name is invalid (a diagnostic was emitted).
    Bad,
}

/// The parameters and return value of a label (as seen from the caller).
#[derive(Debug, Clone, Default)]
pub(super) struct Sig {
    /// Parameter types. Those whose type names were invalid (a diagnostic was emitted) are `None`.
    pub(super) params: Vec<Option<Ty>>,
    /// Parameter names.
    pub(super) names: Vec<String>,
    /// Default values of parameters (constants; `None` for those without defaults or with invalid ones).
    pub(super) defaults: Vec<Option<Value>>,
    pub(super) ret: RetTy,
}

/// The parameter names and default values of the callee (for arranging the arguments of a call;
/// [`Compiler::arrange_args`]).
#[derive(Debug, Clone, Default)]
pub(super) struct ParamSpec {
    pub(super) name: Option<String>,
    pub(super) default: Option<Value>,
}

impl Sig {
    /// Turns parameter names and default values into the form for arranging the arguments of a call.
    pub(super) fn specs(&self) -> Vec<ParamSpec> {
        self.names
            .iter()
            .zip(&self.defaults)
            .map(|(n, d)| ParamSpec {
                name: Some(n.clone()),
                default: d.clone(),
            })
            .collect()
    }
}

impl Sig {
    /// Whether it can only be called with `call` (it has parameters or a return type).
    pub(super) fn callable(&self) -> bool {
        !self.params.is_empty() || !matches!(self.ret, RetTy::None)
    }
}

/// The state while compiling a `choice` (what is decided before reading the bodies).
pub(super) struct ChoiceState {
    /// The sugar is ignored (no code is made).
    pub(super) ignored: bool,
    pub(super) cmd: u32,
    /// Options (bodies are added later).
    pub(super) options: Vec<IrChoiceOpt>,
    pub(super) info: StopInfo,
    /// The length of the reference record before the `choice` (for an ignored sugar, go back to this).
    pub(super) refs_before: (usize, usize),
}

/// The table of declarations of one module.
pub(super) struct ModDecl {
    pub(super) name: String,
    /// Variable name → (sequential number across the whole program, type).
    pub(super) global_map: HashMap<String, (u32, Ty)>,
    /// Label name → sequential number across the whole program.
    pub(super) label_map: HashMap<String, u32>,
    /// `fn` name → the sequential number of the `fn` across the whole program (an index into `Program::fns`).
    pub(super) fn_map: HashMap<String, u32>,
    /// The identities declared in this module (name → display name).
    pub(super) identities: HashMap<String, String>,
    /// All identity declarations of this module, picked up first (for the same name, the first one). A table for resolving
    /// `Identity` constants in the initial values of `save var`s regardless of declaration order.
    pub(super) early_identities: HashMap<String, String>,
    /// Names whose declaration failed. Even if used, "undeclared" is not reported (no cascading).
    pub(super) poisoned: HashSet<String>,
    /// `import`ed modules (name → module index).
    pub(super) imports: HashMap<String, usize>,
    /// `import`ed packages (and namespaces in which the host declared variables).
    pub(super) pkg_imports: HashSet<String>,
    /// Names of `import`s that did not exist (even if used, diagnostics are not repeated).
    pub(super) bad_imports: HashSet<String>,
    /// `struct`s and `enum`s declared in this module (name → index into `Compiler::types`).
    pub(super) type_map: HashMap<String, u32>,
}

/// The state of one label (or `fn`) being compiled: instructions, line numbers, local variables, scopes, loops, return
/// type, and so on.
pub(super) struct LabelCtx {
    pub(super) ir: Ir,
    pub(super) locals: Vec<LocalInfo>,
    /// Name → (slot, type) for each scope. `None` when the type could not be decided (a diagnostic was emitted).
    pub(super) scopes: Vec<HashMap<String, (u32, Option<Ty>)>>,
    pub(super) ids: HashSet<String>,
    pub(super) refs: LabelRefs,
    /// The number of loops currently entered (whether `break` and `continue` can be written).
    pub(super) loop_depth: u32,
    /// The name of this label and its return type.
    pub(super) name: String,
    pub(super) ret: RetTy,
    /// Whether this is the body of a `fn` (quote lines, `jump` and commands with side effects cannot be written, and module
    /// variables cannot be written to).
    pub(super) in_fn: bool,
    /// The record for the liveness check of local variables of function types.
    pub(super) live: lambda::FnLive,
    /// The record for the warning `unused_variable`.
    pub(super) lets: LetUse,
}

/// `let` variables declared in a label or `fn` and the slots read so far.
#[derive(Default)]
pub(super) struct LetUse {
    pub(super) decls: Vec<(u32, String, Pos)>,
    pub(super) read: HashSet<u32>,
}

/// The state of the whole compiler: the registry, diagnostics, declarations per module, the module and label being looked
/// at.
/// One `import` that could be registered.
pub(super) struct ImportSite {
    pub(super) module: usize,
    pub(super) local: String,
    pub(super) path: String,
    pub(super) pos: Pos,
}

pub(super) struct Compiler<'a> {
    pub(super) registry: &'a Registry,
    pub(super) diags: Vec<Diagnostic>,
    pub(super) modules: Vec<ModDecl>,
    /// Module name (path, `story::ch1`) → index (only the first one with a valid name).
    pub(super) module_index: HashMap<String, usize>,
    /// `import`s used (module index, name used in the script). Used for the warning `unused_import`.
    pub(super) used_imports: std::cell::RefCell<HashSet<(usize, String)>>,
    /// `import`s that could be registered (subject to the warning `unused_import`).
    pub(super) import_sites: Vec<ImportSite>,
    /// The last word of the path → index (if the same word appears more than once, the earlier one). Used for the diagnostic
    /// of using a module that is not `import`ed, and for searching candidates.
    pub(super) module_last: HashMap<String, usize>,
    /// The module being read.
    pub(super) cur_mod: usize,
    pub(super) globals: Vec<GlobalInfo>,
    /// The parameters and return values of labels (in order of the sequential number across the whole program).
    pub(super) sigs: Vec<Sig>,
    /// The parameters and return values of `fn`s (in the order of `Program::fns`).
    pub(super) fn_sigs: Vec<Sig>,
    pub(super) commands: Vec<String>,
    /// The names of the effect and query commands used (indices of the `CallEffect` and `CallQuery` instructions).
    pub(super) natives: Vec<String>,
    /// The names of the host-declared variables used (indices of the instructions that read and write host variables).
    pub(super) host_vars: Vec<String>,
    pub(super) cur: Option<LabelCtx>,
    /// String literals passed to `seen` and `visited` (their existence is checked after all labels are compiled).
    pub(super) literals: Vec<LiteralArg>,
    /// The registry's `struct`s and `enum`s (the first `registry_types`), and the `struct`s and `enum`s of all modules (in
    /// module order, then declaration order).
    pub(super) types: Vec<TypeDecl>,
    /// The number of registry types at the start of `types`.
    pub(super) registry_types: usize,
    /// Whether type declarations have been read (until then, the nesting depth of types is not checked).
    pub(super) types_ready: bool,
    /// `map` key types met while reading type declarations (whether they can be keys is checked after the type declarations
    /// have been read). Module index, position, key type.
    pub(super) pending_keys: Vec<(usize, Pos, Ty)>,
    /// Whether text lines without a stable ID are errors (`CompileOptions::require_text_ids`).
    pub(super) require_text_ids: bool,
    /// The lines that can have stable IDs (in script order).
    pub(super) id_sites: Vec<IdSite>,
    /// The types of local variables (module index, position, type).
    pub(super) local_types: Vec<(usize, Pos, String)>,
    /// The state while compiling REPL input (`spec/host/07-repl.md`).
    pub(super) repl: Option<repl::ReplState>,
    /// Bodies whose number of registers exceeded the limit (module index, position). If there are no front-end diagnostics,
    /// they become errors.
    pub(super) reg_overflow: Vec<(usize, Pos)>,
    /// `let` variables never read (module index, position, name). Emitted as warnings only when there are no errors.
    pub(super) unused_vars: Vec<(usize, Pos, String)>,
    /// The number of `fn`s declared in the scripts. The `fn`s of lambdas and higher-order methods come after these.
    pub(super) fn_total: u32,
    /// The `fn`s of lambdas and higher-order methods (from the `fn_total`-th entry of `Program::fns`; `None` until made).
    pub(super) lambdas: Vec<Option<FnCode>>,
    /// The `fn`s made for higher-order methods (indices into `Program::fns`).
    pub(super) hofs: HashMap<lambda::Hof, u32>,
    /// Whether the expression being compiled is a `convert` expression of `migrations`, where scripts' `fn`s, variables, host
    /// commands and the random generator cannot be used (`migrate_impure_expr`).
    pub(super) migrate_expr: bool,
}

impl Compiler<'_> {
    /// In a `convert` expression, reports the use of something that is not allowed and returns true.
    pub(super) fn impure_use(&mut self, pos: Pos, what: &'static str) -> bool {
        if !self.migrate_expr {
            return false;
        }
        self.error(pos, DiagCode::MigrateImpureExpr, vec![("what", s(what))]);
        true
    }
}

/// The target of an assignment or update.
pub(super) struct TargetInfo {
    pub(super) place: PlaceRef,
    /// The type of the target (`None` if it cannot be decided).
    pub(super) ty: Option<Ty>,
    /// The steps that follow fields and indices from the variable.
    pub(super) steps: Vec<PathStep>,
    /// The expressions of the indices and keys of the steps (in step order).
    pub(super) indices: Vec<ExprId>,
}

/// One step after the name of the target (a field or an index).
pub(super) enum PathItem<'e> {
    Field(&'e str, Pos),
    Index(&'e Expr, Pos),
}

/// What a `match` has covered with the arms so far.
/// The bindings of an arm's pattern: the number of the `Payload`, the name and position (`None` for `_`), and the type.
pub(super) type ArmBinding = (u32, Option<(String, Pos)>, Option<Ty>);

/// A record of which values the arms of a `match` covered (used to check exhaustiveness and unreachable arms).
#[derive(Default)]
pub(super) struct Coverage {
    /// There was a `_` arm.
    pub(super) catch_all: bool,
    /// Whether there were `false` / `true` arms.
    pub(super) bools: [bool; 2],
    pub(super) some: bool,
    pub(super) none: bool,
    /// The `enum` variants in the arms.
    pub(super) variants: HashSet<String>,
    /// The `int` and `str` literals in the arms.
    pub(super) lits: HashSet<String>,
}

impl Coverage {
    /// Whether none of the arms from here on can be chosen.
    pub(super) fn complete(&self, ty: Option<&Ty>, decl: Option<&TypeDecl>) -> bool {
        if self.catch_all {
            return true;
        }
        match ty {
            Some(Ty::Bool) => self.bools[0] && self.bools[1],
            Some(Ty::Option(_)) => self.some && self.none,
            Some(Ty::Enum(_)) => match decl.map(|d| &d.kind) {
                Some(TypeKind::Enum { variants }) => {
                    variants.iter().all(|v| self.variants.contains(&v.name))
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// The missing patterns (empty if exhaustive).
    pub(super) fn missing(&self, ty: &Ty, decl: Option<&TypeDecl>) -> Vec<String> {
        if self.catch_all {
            return Vec::new();
        }
        match ty {
            Ty::Bool => [(false, "false"), (true, "true")]
                .iter()
                .filter(|(b, _)| !self.bools[usize::from(*b)])
                .map(|(_, n)| (*n).to_string())
                .collect(),
            Ty::Option(_) => {
                let mut out = Vec::new();
                if !self.none {
                    out.push("None".to_string());
                }
                if !self.some {
                    out.push("Some(_)".to_string());
                }
                out
            }
            Ty::Enum(_) => match decl.map(|d| &d.kind) {
                Some(TypeKind::Enum { variants }) => variants
                    .iter()
                    .filter(|v| !self.variants.contains(&v.name))
                    .map(|v| {
                        let short =
                            crate::value::short_type_name(&decl.expect("宣言").name).to_string();
                        if v.args.is_empty() {
                            format!("{short}.{}", v.name)
                        } else {
                            let holes = vec!["_"; v.args.len()].join(", ");
                            format!("{short}.{}({holes})", v.name)
                        }
                    })
                    .collect(),
                _ => vec!["_".to_string()],
            },
            _ => vec!["_".to_string()],
        }
    }
}

/// How an `enum` variant is written (`TypeName.Variant(expr, ...)`).
pub(super) struct VariantRef<'e> {
    /// The type (an index into `Compiler::types`).
    pub(super) ty: u32,
    pub(super) variant: &'e str,
    pub(super) args: &'e [Expr],
    pub(super) pos: Pos,
}

/// A string literal passed to `seen` or `visited`.
pub(super) struct LiteralArg {
    pub(super) module: usize,
    pub(super) pos: Pos,
    /// Whether it is `seen` (false means `visited`).
    pub(super) seen: bool,
    pub(super) text: String,
}

/// Makes a string diagnostic argument.
pub(super) fn s(v: &str) -> DiagArg {
    DiagArg::Str(v.to_string())
}

/// The diagnostic arguments of a name. If there are close names, adds `suggestions`.
pub(super) fn name_args(name: &str, suggestions: Vec<String>) -> Vec<(&'static str, DiagArg)> {
    let mut args = vec![("name", s(name))];
    if !suggestions.is_empty() {
        args.push(("suggestions", DiagArg::List(suggestions)));
    }
    args
}

/// Whether the return value of a command has a type that can be stored in script variables.
pub(super) fn bindable(t: &Ty) -> bool {
    match t {
        Ty::Int | Ty::Float | Ty::Bool | Ty::Str | Ty::Identity | Ty::Struct(_) | Ty::Enum(_) => {
            true
        }
        Ty::List(i) | Ty::Option(i) => bindable(i),
        Ty::Map(k, v) => bindable(k) && bindable(v),
        _ => false,
    }
}

/// Whether the type is numeric (`int` or `float`). With `int_only`, only `int`.
pub(super) fn numeric_ok(t: &Ty, int_only: bool) -> bool {
    match t {
        Ty::Int => true,
        Ty::Float => !int_only,
        _ => false,
    }
}
