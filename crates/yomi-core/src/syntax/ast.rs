//! The syntax tree.

/// A position in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Pos {
    pub line: u32,
    pub col: u32,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnOp {
    Neg,
    Not,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
}

/// Parts of strings and bodies.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StrPart {
    /// A run of text: the value (escapes resolved) and the characters as written in the source (the input of the position
    /// hash).
    Text(String, String),
    Interp(Expr),
}

/// Expressions.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    Int(i64, Pos),
    Float(f64, Pos),
    Bool(bool, Pos),
    Str(Vec<StrPart>, Pos),
    Name(String, Pos),
    /// A list literal `[a, b]`. The position is `[`.
    List(Vec<Expr>, Pos),
    /// A `map` literal `map { key: value, ... }`. The position is `map`.
    Map(Vec<(Expr, Expr)>, Pos),
    /// `Some(x)`. The position is `Some`.
    Some(Box<Expr>, Pos),
    /// `None`.
    None(Pos),
    /// An index `expr[expr]`. The position is `[`.
    Index(Box<Expr>, Box<Expr>, Pos),
    /// A field `expr.name`. The position is the name.
    Field(Box<Expr>, String, Pos),
    /// A method call `expr.name(expr, ...)`. The position is the name.
    Method(Box<Expr>, String, Vec<Expr>, Pos),
    /// A struct literal `TypeName { name: expr, ... }` (the type name can also be `module.TypeName`). The position is the type
    /// name.
    StructLit(TypeRef, Vec<FieldInit>, Pos),
    /// A variable of another module `module.name`. The position is the position of the module name.
    Qualified(String, String, Pos),
    Unary(UnOp, Box<Expr>, Pos),
    Binary(BinOp, Box<Expr>, Box<Expr>, Pos),
    /// A function call `name(expr, ...)`. The position is the position of the name.
    Call(String, Vec<Expr>, Pos),
    /// A call of a package's query command `package.name(expr, ...)`. The position is the position of the package name.
    QueryCall(String, String, Vec<Expr>, Pos),
    /// A label call `call name(expr, ...)`. Appears only as the initial value of `let` and the right-hand side of `=` (cannot
    /// be written in the middle of an expression). The position is the position of `call`.
    LabelCall(LabelCall),
    /// A command call `@name(expr, ...)`. Can only be written as the whole initial value of `let` or the whole right-hand side
    /// of `=` (to receive the return value).
    Command(CommandCall),
    /// An `@command(...)` written in the middle of an expression. Compilation turns it into `invalid_command_use` (including
    /// `let a = (@f())` enclosed in `( )`).
    MisplacedCommand(CommandCall),
    /// A named argument `name: expr`. Appears only in the arguments of a call. The position is the name.
    Named(String, Box<Expr>, Pos),
    /// The default value of an omitted argument (made only while lining up arguments during compilation; the parser does not
    /// make it).
    Default(crate::value::Value, Pos),
    /// A lambda `|param, ...| expr`.
    Lambda(Box<Lambda>),
}

/// A lambda.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lambda {
    pub params: Vec<LambdaParam>,
    pub body: Expr,
    /// The position of the first `|`.
    pub pos: Pos,
}

/// A parameter of a lambda (the type can be omitted).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LambdaParam {
    pub name: String,
    pub pos: Pos,
    pub ty: Option<TyName>,
}

/// A name that refers to a type (`TypeName` or `module.TypeName`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeRef {
    /// The module name and its position.
    pub module: Option<(String, Pos)>,
    /// The type name and its position.
    pub name: String,
    pub pos: Pos,
}

impl TypeRef {
    /// The form as written (used for diagnostic arguments).
    pub fn written(&self) -> String {
        match &self.module {
            Some((m, _)) => format!("{m}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// One field of a struct literal `name: expr`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FieldInit {
    pub name: String,
    pub pos: Pos,
    pub value: Expr,
}

/// A name that refers to a label (`label` or `module.label`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LabelRef {
    /// The module name and its position.
    pub module: Option<(String, Pos)>,
    /// The name of the label.
    pub name: String,
}

impl LabelRef {
    /// The form as written (used for diagnostic arguments).
    pub fn written(&self) -> String {
        match &self.module {
            Some((m, _)) => format!("{m}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// A label call.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LabelCall {
    pub target: LabelRef,
    pub args: Vec<Expr>,
    pub pos: Pos,
}

/// A command call `@name(expr, ...)` (also `package.name`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CommandCall {
    pub name: String,
    pub args: Vec<Expr>,
    /// The stable ID at the end of the line (only for stop lines).
    pub id: Option<StableId>,
    /// The position right after `)` (where a stable ID is appended; used by `assign-ids`).
    pub id_at: Pos,
    /// The position of `@`.
    pub pos: Pos,
}

impl Expr {
    pub fn pos(&self) -> Pos {
        match self {
            Expr::Int(_, p)
            | Expr::Float(_, p)
            | Expr::Call(_, _, p)
            | Expr::QueryCall(_, _, _, p)
            | Expr::Bool(_, p)
            | Expr::Str(_, p)
            | Expr::Name(_, p)
            | Expr::List(_, p)
            | Expr::Map(_, p)
            | Expr::Some(_, p)
            | Expr::None(p)
            | Expr::Index(_, _, p)
            | Expr::Field(_, _, p)
            | Expr::Method(_, _, _, p)
            | Expr::StructLit(_, _, p)
            | Expr::Qualified(_, _, p)
            | Expr::Unary(_, _, p)
            | Expr::Binary(_, _, _, p)
            | Expr::Named(_, _, p)
            | Expr::Default(_, p) => *p,
            Expr::LabelCall(c) => c.pos,
            Expr::Command(c) | Expr::MisplacedCommand(c) => c.pos,
            Expr::Lambda(l) => l.pos,
        }
    }
}

/// Assignment operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// A type name (as written). For `list<int>`, the name `list` and arguments `[int]`. `module.TypeName` has `.` in the
/// name. A function type `fn(int, str) -> bool` has the name `fn` and arguments `[int, str, bool]` (the last is the
/// return type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TyName {
    pub name: String,
    /// The types inside `<` `>`.
    pub args: Vec<TyName>,
    pub pos: Pos,
}

/// The part after the name of the target of an assignment or update.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PathElem {
    /// `.name`. The position is the name.
    Field(String, Pos),
    /// `[expr]`. The position is `[`.
    Index(Expr, Pos),
}

/// Methods that can be called in a statement that updates a variable (`xs.push(x)`, `xs.insert(i, x)`, and so on).
pub(crate) const STATEMENT_METHODS: &[&str] = &["push", "insert", "remove", "clear", "shuffle"];

/// A pattern of `match`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Pattern {
    /// `_`.
    Wild(Pos),
    Int(i64, Pos),
    Str(String, Pos),
    Bool(bool, Pos),
    /// `None`.
    None(Pos),
    /// `Some(name)` (`_` discards).
    Some(Option<(String, Pos)>, Pos),
    /// An enum variant `name` / `name(name, ...)` (`_` discards).
    Variant {
        name: String,
        binds: Vec<Option<(String, Pos)>>,
        pos: Pos,
    },
}

impl Pattern {
    pub fn pos(&self) -> Pos {
        match self {
            Pattern::Wild(p)
            | Pattern::Int(_, p)
            | Pattern::Str(_, p)
            | Pattern::Bool(_, p)
            | Pattern::None(p)
            | Pattern::Some(_, p) => *p,
            Pattern::Variant { pos, .. } => *pos,
        }
    }
}

/// An arm of `match`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

/// The stable ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StableId {
    pub id: String,
    pub pos: Pos,
}

/// Statements.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Stmt {
    Let {
        name: String,
        /// The position of the name (used for duplicate diagnostics).
        name_pos: Pos,
        ty: Option<TyName>,
        init: Expr,
        pos: Pos,
    },
    Assign {
        /// The module of the assignment target (`module.variable = ...`), with the position of the module. When the variable is a
        /// struct, `variable.field = ...` also goes here (compilation decides which).
        module: Option<(String, Pos)>,
        name: String,
        /// Fields and indices after the name (`c.pos.x`, `xs[i]`).
        path: Vec<PathElem>,
        op: AssignOp,
        value: Expr,
        pos: Pos,
    },
    /// `if` followed by a chain of `else if` (the first of `arms` is the `if`). Without `else`, `else_body` is empty.
    If {
        arms: Vec<(Expr, Vec<Stmt>)>,
        else_body: Vec<Stmt>,
        pos: Pos,
    },
    While {
        cond: Expr,
        body: Vec<Stmt>,
        pos: Pos,
    },
    /// `for name in expr { ... }`, `for name1, name2 in expr { ... }`. The expression is a `list` (with two names, index and
    /// element), or with two names also a `map` (key and value). `key` is the first name when there are two. The position is
    /// `for`.
    For {
        key: Option<(String, Pos)>,
        var: String,
        var_pos: Pos,
        iter: Expr,
        body: Vec<Stmt>,
        pos: Pos,
    },
    /// A statement with a method that updates a variable: `target.push(x)`, `target.remove(i)`, `target.clear()`. The target
    /// is a place that can be on the left of an assignment (`module` and `path` are the same as `Assign`).
    ListMethod {
        module: Option<(String, Pos)>,
        name: String,
        path: Vec<PathElem>,
        method: String,
        method_pos: Pos,
        args: Vec<Expr>,
        pos: Pos,
    },
    /// `match expr { pattern => body, ... }`.
    Match {
        scrutinee: Expr,
        arms: Vec<MatchArm>,
        pos: Pos,
    },
    Break(Pos),
    Continue(Pos),
    /// `jump` (moves to a label).
    Jump {
        target: LabelRef,
        pos: Pos,
    },
    /// A label call whose return value is discarded.
    Call(LabelCall),
    /// `return`. A value is written only when the label has a return type.
    Return {
        value: Option<Expr>,
        pos: Pos,
    },
    End(Pos),
    /// A statement `@command(args)` (the return value is discarded).
    Command(CommandCall),
    Plain {
        parts: Vec<StrPart>,
        raw: String,
        id: Option<StableId>,
        /// The position of the end of the body (where a stable ID is appended).
        id_at: Pos,
        /// When the line starts with `name「` and that name is not an identity, the name (used for the typo warning).
        typo: Option<(String, Pos)>,
        pos: Pos,
    },
    /// A quote line. `parts` is the body including `「` `」`.
    Quote {
        identity: Option<IdentRef>,
        parts: Vec<StrPart>,
        id: Option<StableId>,
        /// The position right after `」` (where a stable ID is appended).
        id_at: Pos,
        pos: Pos,
    },
    /// `choice { ... }`.
    Choice {
        options: Vec<ChoiceOpt>,
        pos: Pos,
    },
}

/// How the identity of a quote line is written (`name` or `module.name`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentRef {
    pub module: Option<(String, Pos)>,
    pub name: String,
    pub pos: Pos,
}

/// One option.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChoiceOpt {
    /// The text including `「` `」`.
    pub parts: Vec<StrPart>,
    /// The position of `once` (if present).
    pub once: Option<Pos>,
    pub cond: Option<Expr>,
    pub id: Option<StableId>,
    /// The position of `=>` (where a stable ID is appended).
    pub id_at: Pos,
    /// The statement to run when chosen.
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

impl ChoiceOpt {
    /// An option with empty contents (filled in while reading, so large values are not moved repeatedly along the recursive
    /// path).
    pub fn empty(pos: Pos) -> Self {
        Self {
            parts: Vec::new(),
            once: None,
            cond: None,
            id: None,
            id_at: pos,
            body: Vec::new(),
            pos,
        }
    }
}

/// The declaration of a module variable (`save var` and `persistent var`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VarDecl {
    pub name: String,
    pub ty: TyName,
    pub init: Expr,
    /// Whether it is a `persistent var` (false means `save var`).
    pub persistent: bool,
    pub pos: Pos,
}

/// A parameter of a label.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Param {
    pub name: String,
    pub pos: Pos,
    pub ty: TyName,
    /// The default value (`= constant`).
    pub default: Option<Expr>,
}

/// The declaration of a label (`fn` declarations have the same form).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LabelDecl {
    pub name: String,
    pub params: Vec<Param>,
    /// The return type (`-> type`).
    pub ret: Option<TyName>,
    pub body: Vec<Stmt>,
    pub pos: Pos,
}

/// A `struct` / `enum` declaration.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TypeDeclAst {
    pub name: String,
    pub pos: Pos,
    pub kind: TypeDeclKind,
}

/// The contents of the declaration.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TypeDeclKind {
    Struct(Vec<(String, Pos, TyName)>),
    Enum(Vec<VariantAst>),
}

/// Variants of an `enum`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VariantAst {
    pub name: String,
    pub pos: Pos,
    pub args: Vec<TyName>,
}

/// One name of an `import`. `import a::{b, c as d}` expands to two: `a::b` and `a::c` (alias `d`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Import {
    /// The path of the module (or package), joined with `::` (`story::ch1`).
    pub path: String,
    /// The name used in the script. The alias if there is one, otherwise the last word of the path.
    pub local: String,
    /// Whether an alias was given with `as`.
    pub aliased: bool,
    pub pos: Pos,
}

impl Import {
    /// The form written in the script (`story::ch1`, or `story::ch1 as c` with an alias).
    pub fn shown(&self) -> String {
        if self.aliased {
            format!("{} as {}", self.path, self.local)
        } else {
            self.path.clone()
        }
    }
}

/// `identity name = "display name"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentityDecl {
    pub name: String,
    pub display: String,
    pub pos: Pos,
}

/// A module.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Module {
    pub imports: Vec<Import>,
    pub identities: Vec<IdentityDecl>,
    pub vars: Vec<VarDecl>,
    pub labels: Vec<LabelDecl>,
    /// `fn` declarations (in the order written; same form as labels).
    pub fns: Vec<LabelDecl>,
    /// `struct` and `enum` declarations (in the order written).
    pub types: Vec<TypeDeclAst>,
    /// Names of declarations that failed to parse (`let`, `save var`, labels). Type checking does not report these names as
    /// "undeclared" (to avoid cascading errors).
    pub poisoned: std::collections::HashSet<String>,
    /// Held only by the REPL module: the order of declarations (`(kind, index within that kind)`; kinds are 1: identity,
    /// 2: `fn`, 3: label). It gathers declarations from multiple inputs whose positions are per input, so the declaration
    /// order is kept here.
    pub repl_order: Vec<(u8, usize)>,
}
