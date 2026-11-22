//! Intermediate representation (IR). The front end (parsing, type checking, name resolution) makes it, and a backend
//! turns it into instructions.
//!
//! Types and names are resolved and checked (diagnostics are emitted by the front end). The control structure is kept,
//! without jump target numbers (the backend decides jump targets). Each node has the line number to attach to the
//! instructions it emits.
//!
//! The order of evaluation is determined by the order here (arguments, elements and operands in the order written).
//! Backends do not reorder side-effecting expressions.

use crate::pool::{Idx, Pool};
use crate::program::{Action, ArithOp, PathStep, PlaceRef, StopInfo};
use crate::syntax::ast::{BinOp, UnOp};
use crate::value::Value;

pub(crate) type ExprId = Idx<Expr>;
pub(crate) type StmtId = Idx<Stmt>;

/// The IR tables of one label or `fn`.
#[derive(Debug, Default)]
pub(crate) struct Ir {
    pub exprs: Pool<Expr>,
    pub stmts: Pool<Stmt>,
}

impl Ir {
    pub(crate) fn expr(&mut self, kind: ExprKind, line: u32) -> ExprId {
        self.exprs.push(Expr { kind, line })
    }

    pub(crate) fn stmt(&mut self, kind: StmtKind, line: u32) -> StmtId {
        self.stmts.push(Stmt { kind, line })
    }
}

/// The body of one label or `fn`.
#[derive(Debug)]
pub(crate) struct Body {
    pub ir: Ir,
    pub stmts: Vec<StmtId>,
    pub tail: Tail,
    /// Whether this is the body of a `fn` (`return` behaves differently).
    pub in_fn: bool,
    /// The number of local variables (parameters, `let`, bindings, and hidden variables of `for`).
    pub locals: u32,
    /// The line of the final instruction.
    pub line: u32,
}

/// The end of a body (when execution proceeds past the last statement).
#[derive(Debug)]
pub(crate) enum Tail {
    /// Returns to the caller (for a label with no caller, same as `end`).
    Return,
    /// Ends the script (a label with a return type does not reach its end).
    End,
    /// The end of REPL input. For expression input, keeps its value.
    Repl(Option<ExprId>),
}

/// One expression.
#[derive(Debug)]
pub(crate) struct Expr {
    pub kind: ExprKind,
    pub line: u32,
}

/// The kind of expression.
#[derive(Debug)]
pub(crate) enum ExprKind {
    /// An expression that produced a diagnostic. No program is made, so it is not turned into code.
    Error,
    Const(Value),
    /// The value of a variable.
    Var(PlaceRef),
    /// The `field`-th field of a `struct` value.
    Field {
        base: ExprId,
        field: u32,
    },
    /// The `id` (false) or `name` (true) of an `Identity`.
    IdField {
        base: ExprId,
        name: bool,
    },
    Unary {
        op: UnOp,
        operand: ExprId,
    },
    /// A binary operation. `&&` and `||` short-circuit. `ty` is the type of the operands.
    Binary {
        op: BinOp,
        ty: Operand,
        lhs: ExprId,
        rhs: ExprId,
    },
    List(Vec<ExprId>),
    /// Pairs of key and value (in the order written).
    Map(Vec<(ExprId, ExprId)>),
    Some(ExprId),
    /// A `struct` value. `fields` are the values in the order written, and `order` is which field in the declaration each
    /// value is.
    Struct {
        ty: u32,
        order: Vec<u32>,
        fields: Vec<ExprId>,
    },
    /// An `enum` value with arguments (those without arguments are `Const`).
    Enum {
        ty: u32,
        variant: u32,
        args: Vec<ExprId>,
    },
    /// An index (`int` for `list`, the key for `map`).
    Index {
        base: ExprId,
        index: ExprId,
    },
    Method {
        method: Method,
        base: ExprId,
        args: Vec<ExprId>,
    },
    Builtin {
        builtin: Builtin,
        args: Vec<ExprId>,
    },
    /// A string joining string parts. If `record` is true, the values of `${expr}` are added to the current record of the
    /// body (the body of a stop point).
    Text {
        parts: Vec<TextPiece>,
        record: bool,
    },
    /// A call of a `fn`.
    CallFn {
        f: u32,
        args: Vec<ExprId>,
    },
    /// A call of a function value. The function value is evaluated first, then the arguments.
    CallValue {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    /// Makes a function value (`f` is an index into `Program::fns`, `captures` are the captured values, evaluated in the order
    /// written).
    MakeFn {
        f: u32,
        captures: Vec<ExprId>,
    },
    /// A call of a label (`call`). `hash` is the content hash of the call position (used to find the position on load).
    CallLabel {
        label: u32,
        args: Vec<ExprId>,
        hash: u64,
    },
    /// A query command. `native` is an index into `Program::natives`.
    CallQuery {
        native: u32,
        args: Vec<ExprId>,
    },
    /// An effect command.
    CallEffect {
        native: u32,
        args: Vec<ExprId>,
    },
    /// A stop point (a stop command, plain line or quote line). Has a value only when the return value is received
    /// (`info.binds`).
    Stop(Box<Stop>),
}

/// A stop point.
#[derive(Debug)]
pub(crate) struct Stop {
    /// An index into `Program::commands`.
    pub cmd: u32,
    /// A stable ID to mark as passed (recorded before computing the arguments).
    pub mark_seen: Option<String>,
    pub args: Vec<StopArg>,
    /// Information about a stop point. The instruction positions (`args_pc`, `pc`) are decided by the backend.
    pub info: StopInfo,
}

/// The arguments of a stop point.
#[derive(Debug)]
pub(crate) struct StopArg {
    pub expr: ExprId,
    /// Whether it is a body (a string whose `${expr}` values are recorded).
    pub text: bool,
}

/// One statement.
#[derive(Debug)]
pub(crate) struct Stmt {
    pub kind: StmtKind,
    pub line: u32,
}

/// The kind of statement.
#[derive(Debug)]
pub(crate) enum StmtKind {
    /// Evaluates an expression (the value is discarded).
    Eval(ExprId),
    /// Stores into a variable.
    Store {
        place: PlaceRef,
        value: ExprId,
    },
    /// `variable op= value` (the value is evaluated first). `ty` is the type of the variable and the value.
    Compound {
        place: PlaceRef,
        op: ArithOp,
        ty: Operand,
        value: ExprId,
    },
    /// Updates in place the place reached from a variable through fields and indices. Evaluation order: indices (`indices`),
    /// then the update values (`operands`).
    Mutate {
        place: PlaceRef,
        path: Vec<PathStep>,
        indices: Vec<ExprId>,
        operands: Vec<ExprId>,
        action: Action,
    },
    /// `if`, `else if`, `else`.
    If {
        arms: Vec<IfArm>,
        else_body: Vec<StmtId>,
    },
    While {
        cond: ExprId,
        body: Vec<StmtId>,
    },
    /// `for`. `list` and `idx` are hidden local variables, `var` is the variable that receives the element (the value for a
    /// `map`), and `key` is the second variable (the index for a `list`, the key for a `map`). What `list` and `idx` hold
    /// depends on `kind`.
    For {
        iter: ExprId,
        list: u32,
        idx: u32,
        var: u32,
        key: Option<u32>,
        kind: ForKind,
        body: Vec<StmtId>,
    },
    /// `match`. Arms are tested from top to bottom.
    Match {
        scrutinee: ExprId,
        arms: Vec<MatchArm>,
    },
    Choice(Box<Choice>),
    Break,
    Continue,
    /// Moves to the start of another label.
    JumpLabel(u32),
    End,
    /// Returns to the caller.
    Return(Option<ExprId>),
}

/// One arm of an `if`. `line` is the line of the branch instruction of the condition.
#[derive(Debug)]
pub(crate) struct IfArm {
    pub cond: ExprId,
    pub body: Vec<StmtId>,
    pub line: u32,
}

/// One arm of a `match`.
#[derive(Debug)]
pub(crate) struct MatchArm {
    /// The condition to enter the arm (`None` for `_`), and its line.
    pub test: Option<(PatTest, u32)>,
    /// The bindings to extract (the index of the content, the local variable, the line).
    pub binds: Vec<(u32, u32, u32)>,
    pub body: Vec<StmtId>,
    pub line: u32,
}

/// A pattern test.
#[derive(Debug)]
pub(crate) enum PatTest {
    Lit(Value),
    Some,
    None,
    /// A variant of an `enum` (by name).
    Variant(String),
}

/// `choice`.
#[derive(Debug)]
pub(crate) struct Choice {
    /// An index into `Program::commands`.
    pub cmd: u32,
    pub options: Vec<ChoiceOpt>,
    /// Information about a stop point. Instruction positions are decided by the backend.
    pub info: StopInfo,
}

/// One option.
#[derive(Debug)]
pub(crate) struct ChoiceOpt {
    /// The stable ID of a `once` option.
    pub once: Option<String>,
    pub cond: Option<ExprId>,
    /// The text of the option (a body).
    pub text: ExprId,
    /// The value of the stable ID (`Option<str>`).
    pub id: Value,
    pub body: Vec<StmtId>,
    pub line: u32,
}

/// The type of an operand of an operation (to choose typed instructions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Operand {
    Int,
    Float,
    /// Other than `int` and `float` (comparison with `==` and `!=`).
    Other,
}

impl Operand {
    pub(crate) fn of(ty: Option<&crate::value::Ty>) -> Self {
        match ty {
            Some(crate::value::Ty::Int) => Operand::Int,
            Some(crate::value::Ty::Float) => Operand::Float,
            _ => Operand::Other,
        }
    }
}

/// A part of a string.
#[derive(Debug)]
pub(crate) enum TextPiece {
    Lit(String),
    /// `${expr}`. `to_str` tells whether to turn a non-`str` value into a string.
    Interp {
        expr: ExprId,
        to_str: bool,
    },
}

/// A method of `list` or `map` called inside an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Method {
    Len,
    IsEmpty,
    ListContains,
    /// `xs.sort()`: a stable ascending copy of the list (the elements are the keys).
    ListSort,
    /// `xs.reverse()`: a reversed copy.
    ListReverse,
    /// `xs.sum()` of an `int` list.
    ListSum,
    /// `xs.sum()` of a `float` list.
    ListSumFloat,
    /// Stable ascending copy of `base`, ordered by the `keys` list (the first argument).
    ListSortBy,
    /// The element of `base` with the least key in the `keys` list (the first argument), as an `Option`.
    ListMinBy,
    /// The element of `base` with the greatest key in the `keys` list (the first argument), as an `Option`.
    ListMaxBy,
    MapHas,
    MapGet,
    MapKeys,
    MapValues,
}

/// How a `for` iterates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForKind {
    /// Iterates a `list`. `list` is the copied `list`, `idx` is the next position.
    List,
    /// Iterates a `map` (in insertion order). `list` is the copied `map` (with holes compacted), `idx` is the next position.
    Map,
    /// Iterates `range(a, b)` without building a list. `iter` is `a` and `end` is `b`. `list` holds the value of `b`, `idx`
    /// the next value.
    Range { end: ExprId },
}

/// Built-in functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Builtin {
    ToInt,
    /// `float(x)` (converts an `int` to a `float`).
    ToFloat,
    Seen,
    Visited,
    Random,
    /// `random_float()` (a random number in [0, 1)).
    RandomFloat,
    Range,
}
