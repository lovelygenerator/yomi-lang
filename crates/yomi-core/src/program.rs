//! A compiled program (bytecode).

use std::sync::Arc;

use crate::pool::{Idx, Pool};
use crate::value::{Ty, TypeDecl, Value};

/// The reference to the program that a `Vm` holds. Either a `Program` borrowed from the host, or an extended program
/// built (and owned) by the REPL (section 7.8 of `spec/host/07-repl.md`).
#[derive(Debug, Clone)]
pub(crate) enum ProgRef<'a> {
    /// Borrows the `Program` given by the host.
    Borrowed(&'a Program),
    /// An extended program built by the REPL (shared between copies).
    Shared(Arc<Program>),
}

impl std::ops::Deref for ProgRef<'_> {
    type Target = Program;

    fn deref(&self) -> &Program {
        match self {
            ProgRef::Borrowed(p) => p,
            ProgRef::Shared(p) => p,
        }
    }
}

/// A register number (an index into the frame's array, where temporaries follow the local variables).
pub(crate) type Reg = u16;

/// A register-based instruction. Values are passed through registers (`Reg`). Variable-length data is held as indices
/// into the tables of [`RegCode`]. The place for return values and resume values is the `dst` of the instruction that
/// is returned to (`Call`, `CallFn`, `Stop`, `ChoiceStop`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RegOp {
    Int {
        dst: Reg,
        n: i64,
    },
    Const {
        dst: Reg,
        k: Idx<Value>,
    },
    /// Copies to a temporary (the size is not counted).
    Move {
        dst: Reg,
        src: Reg,
    },
    /// Copies into a local variable (the size is counted).
    Set {
        dst: Reg,
        src: Reg,
    },
    /// Moves the value of a temporary into a local variable (the size is counted; the temporary becomes `()`).
    SetMove {
        dst: Reg,
        src: Reg,
    },
    LoadGlobal {
        dst: Reg,
        g: u32,
    },
    StoreGlobal {
        g: u32,
        src: Reg,
    },
    LoadHost {
        dst: Reg,
        h: u32,
    },
    StoreHost {
        h: u32,
        src: Reg,
    },
    /// Reads the value reached from a variable through `struct` fields (without copying the whole value of the variable).
    LoadField {
        dst: Reg,
        place: PlaceRef,
        path: Idx<Box<[u32]>>,
    },
    GetField {
        dst: Reg,
        src: Reg,
        field: u32,
    },
    IdField {
        dst: Reg,
        src: Reg,
        name: bool,
    },
    AddI {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    SubI {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    MulI {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    DivI {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    RemI {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    AddIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    SubIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    MulIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    DivIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    RemIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    LtI {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    LeI {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    LtIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    LeIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    GtIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    GeIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    EqIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    NeIK {
        dst: Reg,
        a: Reg,
        k: i64,
    },
    /// `float` arithmetic (no `%`).
    ArithF {
        op: ArithOp,
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    /// `float` ordering comparison (`a < b`, `a <= b`; `>` and `>=` swap the operands).
    LtF {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    LeF {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    /// Equality of values (any type).
    Eq {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Ne {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    /// Negation (`int` and `float`).
    Neg {
        dst: Reg,
        a: Reg,
    },
    Not {
        dst: Reg,
        a: Reg,
    },
    ToInt {
        dst: Reg,
        a: Reg,
    },
    ToFloat {
        dst: Reg,
        a: Reg,
    },
    ToStr {
        dst: Reg,
        a: Reg,
    },
    /// Concatenates the strings in the `n` registers starting at `start`.
    Concat {
        dst: Reg,
        start: Reg,
        n: u16,
    },
    Range {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Random {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    RandomFloat {
        dst: Reg,
    },
    Jump {
        to: u32,
    },
    JumpIfFalse {
        cond: Reg,
        to: u32,
    },
    JumpIfTrue {
        cond: Reg,
        to: u32,
    },
    /// Integer comparison and conditional jump (jumps to `to` if `a < b` is false). The form used for the conditions of `if`
    /// and `while` that does not put the comparison result in a temporary.
    JumpUnlessLtI {
        a: Reg,
        b: Reg,
        to: u32,
    },
    JumpUnlessLeI {
        a: Reg,
        b: Reg,
        to: u32,
    },
    JumpUnlessLtIK {
        a: Reg,
        k: i64,
        to: u32,
    },
    JumpUnlessLeIK {
        a: Reg,
        k: i64,
        to: u32,
    },
    JumpUnlessGtIK {
        a: Reg,
        k: i64,
        to: u32,
    },
    JumpUnlessGeIK {
        a: Reg,
        k: i64,
        to: u32,
    },
    JumpUnlessEqIK {
        a: Reg,
        k: i64,
        to: u32,
    },
    JumpUnlessNeIK {
        a: Reg,
        k: i64,
        to: u32,
    },
    MakeList {
        dst: Reg,
        start: Reg,
        n: u16,
    },
    MakeSome {
        dst: Reg,
        a: Reg,
    },
    MakeStruct {
        dst: Reg,
        start: Reg,
        ty: u32,
        order: Idx<Vec<u32>>,
    },
    MakeEnum {
        dst: Reg,
        start: Reg,
        n: u16,
        ty: u32,
        variant: u32,
    },
    /// Makes a `map` from the `2 * n` registers starting at `start` (key then value).
    MakeMap {
        dst: Reg,
        start: Reg,
        n: u16,
    },
    Index {
        dst: Reg,
        base: Reg,
        index: Reg,
    },
    MapGet {
        dst: Reg,
        map: Reg,
        key: Reg,
    },
    MapHas {
        dst: Reg,
        map: Reg,
        key: Reg,
    },
    MapKeys {
        dst: Reg,
        map: Reg,
    },
    MapValues {
        dst: Reg,
        map: Reg,
    },
    Len {
        dst: Reg,
        a: Reg,
    },
    IsEmpty {
        dst: Reg,
        a: Reg,
    },
    ListContains {
        dst: Reg,
        list: Reg,
        item: Reg,
    },
    /// A stable ascending copy of `list`, ordered by `keys` (the same register as `list` for a plain sort).
    ListSortBy {
        dst: Reg,
        list: Reg,
        keys: Reg,
    },
    ListReverse {
        dst: Reg,
        list: Reg,
    },
    /// The total of `list`. `float` is whether the elements are `float` (the total of an empty list is `0.0` or `0`).
    ListSum {
        dst: Reg,
        list: Reg,
        float: bool,
    },
    /// The first element of `list` with the least (or greatest, if `max`) key in `keys`, as an `Option`.
    ListExtreme {
        dst: Reg,
        list: Reg,
        keys: Reg,
        max: bool,
    },
    /// Moves to `next` if `src` is not equal to the constant.
    MatchLit {
        src: Reg,
        k: Idx<Value>,
        next: u32,
    },
    MatchSome {
        src: Reg,
        next: u32,
    },
    MatchNone {
        src: Reg,
        next: u32,
    },
    MatchVariant {
        src: Reg,
        name: Idx<String>,
        next: u32,
    },
    /// Discards a temporary (discards the subject of a `match` on entering an arm).
    Clear {
        r: Reg,
    },
    /// The `i`-th content of `src` (a `Some` or an `enum`).
    Payload {
        dst: Reg,
        src: Reg,
        i: u32,
    },
    /// The next element of a `for` (`list`, `idx` and `var` are local variables). Moves to `end` when finished.
    ForNext {
        list: Reg,
        idx: Reg,
        var: Reg,
        end: u32,
    },
    /// Same as `ForNext`, and also puts the index in `key` (`for i, x in xs`).
    ForNextAt {
        list: Reg,
        idx: Reg,
        var: Reg,
        key: Reg,
        end: u32,
    },
    /// Compacts the holes left by removals in the copy of the `map` that a `for` iterates.
    MapCompact {
        map: Reg,
    },
    /// Puts the pair at position `idx` of the `map` into `key` and `var`, and advances `idx`. Moves to `end` if there is none.
    ForMapNext {
        map: Reg,
        idx: Reg,
        key: Reg,
        var: Reg,
        end: u32,
    },
    /// `for i in range(a, b)`: if `idx` is less than `limit`, puts it in `var` and advances; otherwise moves to `end`.
    ForRange {
        idx: Reg,
        limit: Reg,
        var: Reg,
        end: u32,
    },
    /// Updates a place in place. Starting at `start`, the indices and keys (in the order of the path's `Index` and `Key`)
    /// come first, followed by the update values.
    Mutate {
        place: PlaceRef,
        plan: Idx<(Vec<PathStep>, Action)>,
        start: Reg,
    },
    ArgsBegin,
    TextBegin,
    /// Records the string in `src` as the value of a `${expr}` of the current body.
    Capture {
        src: Reg,
    },
    /// An effect command. If it has a return value, puts it in `dst`.
    CallEffect {
        dst: Reg,
        start: Reg,
        n: u16,
        native: u32,
    },
    CallQuery {
        dst: Reg,
        start: Reg,
        n: u16,
        native: u32,
    },
    Seen {
        dst: Reg,
        a: Reg,
    },
    Visited {
        dst: Reg,
        a: Reg,
    },
    OnceAvailable {
        dst: Reg,
        key: Idx<String>,
    },
    MarkSeen {
        key: Idx<String>,
    },
    MarkOnce {
        key: Idx<String>,
    },
    ChoiceBegin,
    ChoiceItem {
        text: Reg,
        id: Reg,
        orig: u32,
    },
    JumpLabel {
        label: u32,
    },
    /// Calls a label. The `n` registers from `start` are the arguments. The return value goes into `dst`.
    Call {
        dst: Reg,
        start: Reg,
        n: u16,
        label: u32,
        site: u32,
    },
    CallFn {
        dst: Reg,
        start: Reg,
        n: u16,
        f: u32,
    },
    /// Calls a function value (`callee`). The `n` registers from `start` are the arguments. The return value goes into `dst`.
    CallValue {
        dst: Reg,
        callee: Reg,
        start: Reg,
        n: u16,
    },
    /// Makes a function value. The `n` registers from `start` are the captured values.
    MakeFn {
        dst: Reg,
        start: Reg,
        n: u16,
        f: u32,
    },
    ReturnFn {
        src: Option<Reg>,
    },
    Return {
        src: Option<Reg>,
    },
    /// Stops at a stop command. The `n` registers from `start` are the arguments. The resume value goes into `dst` (only for
    /// stop points that receive it).
    Stop {
        dst: Reg,
        start: Reg,
        n: u16,
        cmd: u32,
        stop: u32,
    },
    /// Stops at a choice. On resume, the number of the chosen option (in the order written) goes into `dst`.
    ChoiceStop {
        dst: Reg,
        cmd: u32,
        stop: u32,
    },
    /// Moves to the position numbered by `src`.
    JumpTable {
        src: Reg,
        table: Idx<Vec<u32>>,
    },
    /// The end of REPL input. For expression input, keeps its value (`src`).
    ReplDone {
        src: Option<Reg>,
    },
    End,
}

/// The register-based code of one label or `fn`.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct RegCode {
    pub ops: Vec<RegOp>,
    /// The number of registers (local variables and temporaries).
    pub regs: u32,
    /// The number of local variables (registers after these are temporaries).
    pub vars: u32,
    pub consts: Pool<Value>,
    pub paths: Pool<Box<[u32]>>,
    pub orders: Pool<Vec<u32>>,
    pub names: Pool<String>,
    pub plans: Pool<(Vec<PathStep>, Action)>,
    pub tables: Pool<Vec<u32>>,
}

/// The variable of the place to update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceRef {
    Local(u32),
    Global(u32),
    Host(u32),
}

/// One step of the path to a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathStep {
    /// The `i`-th field of a `struct`.
    Field(u32),
    /// An index into a `list` (the value is taken from the stack).
    Index,
    /// A key of a `map` (the value is taken from the stack). A `Set` at the last step appends the key if it is missing.
    Key,
}

/// The kind of update to a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    /// Assignment.
    Set,
    /// `place op= value` (`op` is `ArithOp::Add` and so on).
    Arith(ArithOp),
    /// Appends to the end of a `list`.
    Push,
    /// Inserts at an index of a `list` (`0 <= i <= len`). The operands are the value (top) and the index (below it).
    Insert,
    /// Removes the element at an index of a `list` (for a `map`, removes the pair with the key; does nothing if missing).
    Remove,
    /// Empties a `list` or `map`.
    Clear,
    /// Shuffles the elements of a `list` with the core's random generator (Fisher–Yates).
    Shuffle,
}

/// The operation of a compound assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// The kind of stop point: a sugar (plain line, quote line, choice) or a command written with `@`. Text tables
/// (`spec/host/05-texts.md`) replace only the bodies of sugar.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StopKind {
    /// A stop command called with `@` (its arguments are not replaced).
    #[default]
    Command,
    /// A plain line (argument 0 is the body).
    Plain,
    /// A quote line (argument 1 is the body).
    Quote,
    /// A choice (argument 0 is the `list` of option texts, argument 1 the `list` of stable IDs).
    Choice,
}

impl StopKind {
    /// The stable English name.
    pub fn code(self) -> &'static str {
        match self {
            StopKind::Command => "command",
            StopKind::Plain => "plain",
            StopKind::Quote => "quote",
            StopKind::Choice => "choice",
        }
    }
}

/// A part of the original text of a body (`${expr}` is only a number; the expression itself is not kept).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextPart {
    /// Characters (after unescaping).
    Lit(String),
    /// One `${expr}`.
    Interp,
}

/// Information about a stop point.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StopInfo {
    /// The kind of sugar (`@` commands are `Command`).
    pub kind: StopKind,
    /// The original text of the body of a plain or quote line (empty otherwise). For a command with `text_arg`, the original
    /// text of that argument.
    pub text: Vec<TextPart>,
    /// A stop point of a command whose arguments can be translated by a text table (when an argument that is a `text_param`
    /// in the registry is written as a string literal). The argument number, and its number in the record of `${expr}` values
    /// (`StopRecord::values`).
    pub text_arg: Option<TextArg>,
    /// The start of the instructions that compute the arguments.
    pub args_pc: u32,
    /// The position of the stopping instruction.
    pub pc: u32,
    /// The stable ID.
    pub id: Option<String>,
    /// The content hash (command name + original text of the arguments).
    pub hash: u64,
    /// Line.
    pub line: u32,
    /// For a choice stop point, information about its options.
    pub choice: Option<ChoiceInfo>,
    /// Whether the stop point receives the return value in a variable (`let name = @command(...)`, `name = @command(...)`).
    /// If true, the resume value (`resume`) is pushed onto the stack, and the instruction after the stop stores it in the
    /// variable.
    pub binds: bool,
}

/// An argument of a command that can be translated by a text table (section 5.2a of `spec/host/05-texts.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TextArg {
    /// The argument number.
    pub arg: u32,
    /// The number in the record of `${expr}` values (only string literal arguments are counted).
    pub values: u32,
}

/// One identity declaration (the display name can be translated by a text table; section 5.2a of
/// `spec/host/05-texts.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdentityInfo {
    /// The declaring module (an index into `Program::modules`).
    pub module: u32,
    /// The declared name (`案内人`).
    pub name: String,
    /// The display name (original text).
    pub display: String,
    /// The display name as parts of the original text of a body (has no `${expr}`).
    pub parts: Vec<TextPart>,
}

/// Information about the options of a choice stop point (in the order written).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChoiceInfo {
    pub options: Vec<ChoiceOptInfo>,
}

/// Information about one option.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ChoiceOptInfo {
    /// The stable ID.
    pub id: Option<String>,
    /// The original text of the option.
    pub text: Vec<TextPart>,
    /// The line of the option.
    pub line: u32,
}

/// A local variable.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LocalInfo {
    pub name: String,
    pub ty: Ty,
}

/// Information about where a `call` was placed (used on load to find the position of the caller).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CallSite {
    /// The position of the `Call` instruction. Execution returns to the one after it.
    pub pc: u32,
    /// The content hash (`call` + how the called label is written + original text of the arguments).
    pub hash: u64,
    /// The called label (a sequential number across the whole program).
    pub target: u32,
    /// Line.
    pub line: u32,
}

/// The code of a label.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LabelCode {
    /// The module it belongs to (an index into `Program::modules`).
    pub module: u32,
    pub name: String,
    pub reg: Box<RegCode>,
    /// The line number of each instruction.
    pub lines: Vec<u32>,
    /// Local variables. The first `params` are the parameters.
    pub locals: Vec<LocalInfo>,
    /// The number of parameters.
    pub params: u32,
    /// The return type.
    pub ret: Option<Ty>,
    pub stops: Vec<StopInfo>,
    pub calls: Vec<CallSite>,
}

impl LabelCode {
    /// Whether the label can only be called with `call` (it has parameters or a return type).
    pub fn callable(&self) -> bool {
        self.params > 0 || self.ret.is_some()
    }
}

/// The code of a `fn`. Unlike a label, it has neither stop points nor `call` positions (it does not go into saves).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FnCode {
    /// The module it belongs to (an index into `Program::modules`).
    pub module: u32,
    pub name: String,
    pub reg: Box<RegCode>,
    pub lines: Vec<u32>,
    /// Local variables. The first `params` are the parameters, followed by `captures` captured values.
    pub locals: Vec<LocalInfo>,
    /// The number of parameters.
    pub params: u32,
    /// The number of captured values (only for lambdas; copied from the function value).
    pub captures: u32,
    /// The return type.
    pub ret: Option<Ty>,
}

/// A module variable.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GlobalInfo {
    /// The module it belongs to (an index into `Program::modules`).
    pub module: u32,
    /// Whether it is a `persistent var` (false means `save var`). It goes into persistent data, not into saves.
    pub persistent: bool,
    pub name: String,
    pub ty: Ty,
    pub init: Value,
}

/// The compiled `migrate` blocks and the frozen schemas they use (`spec/language/07-migrate.md`).
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct MigrateCode {
    /// In version order.
    pub blocks: Vec<MigrateBlock>,
    /// The frozen schemas, by version.
    pub schemas: std::collections::BTreeMap<u32, FrozenSchema>,
}

/// One `migrate N { … }` block.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MigrateBlock {
    pub version: u32,
    pub ops: Vec<MigrateOp>,
}

/// One statement of a `migrate` block. Variable names are `module.variable`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MigrateOp {
    Rename {
        old: String,
        new: String,
        persistent: bool,
    },
    Convert {
        old: String,
        new: String,
        persistent: bool,
        /// The types as written, without version prefixes.
        old_ty: String,
        new_ty: String,
        /// The hidden label that evaluates the expression (parameter 0 is the old value; the result is left on the stack).
        label: usize,
        /// Whether the new side is a frozen schema (otherwise the current scripts).
        new_frozen: bool,
    },
    Drop {
        old: String,
        persistent: bool,
    },
}

/// A frozen schema as the program keeps it: the shapes of the declarations (as in `TypeDecl::signature`), for expanding the
/// types of variables, and the names of the types that have a versioned copy (`v2::game.Item`) for running `convert`.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct FrozenSchema {
    pub decls: Vec<(String, String)>,
    pub versioned: Vec<String>,
}

/// A compiled program (one or more modules).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The names of the modules (in the order given).
    pub(crate) modules: Vec<String>,
    /// The variables of all modules (in module order, then declaration order).
    pub(crate) globals: Vec<GlobalInfo>,
    /// The labels of all modules (in module order, then declaration order).
    pub(crate) labels: Vec<LabelCode>,
    /// The `fn`s of all modules (in module order, then declaration order).
    pub(crate) fns: Vec<FnCode>,
    /// The names of the stop commands used.
    pub(crate) commands: Vec<String>,
    /// The names of the effect and query commands used.
    pub(crate) natives: Vec<String>,
    /// The names of the host-declared variables used.
    pub(crate) host_vars: Vec<String>,
    /// The `struct`s and `enum`s declared by users (in module order, then declaration order).
    pub(crate) types: Vec<TypeDecl>,
    /// The lines that can have stable IDs (in script order; used by `assign-ids`).
    pub(crate) id_sites: Vec<IdSite>,
    /// Identity declarations (in module order, then declaration order).
    pub(crate) identities: Vec<IdentityInfo>,
    /// The compiled sources (in the same order, from the start of `modules`). Kept so that the REPL can build an extended
    /// program with added declarations. In the REPL's extended program, the `<repl>` module is not included (shorter than
    /// `modules`).
    pub(crate) sources: Vec<String>,
    pub(crate) schema_version: u32,
    pub(crate) min_save_version: u32,
    pub(crate) migrate: MigrateCode,
}

/// One line that can have a stable ID (a plain line, quote line, option text or stop command line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdSite {
    /// The module it belongs to (an index into `Program::modules`).
    pub module: u32,
    /// The name of the label it belongs to.
    pub label: String,
    /// The kind (option texts are `Choice`).
    pub kind: StopKind,
    /// The stable ID written.
    pub id: Option<String>,
    /// When there is no ID, the position to append `#id` (line and column; columns are 1-based in characters).
    pub line: u32,
    pub col: u32,
}

impl Program {
    /// The schema version of the scripts (the largest `N` of the `migrate N` blocks; 1 when there are none).
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    /// The oldest save schema version that can still be loaded (`min_save_version`; 1 when not written).
    pub fn min_save_version(&self) -> u32 {
        self.min_save_version
    }

    /// Declarations of `struct`s and `enum`s declared by users (in module order, then declaration order).
    pub fn types(&self) -> &[TypeDecl] {
        &self.types
    }

    /// Keys and original text parts of the bodies to be replaced. In script order: display names of identities
    /// (`identity:module.name`, in declaration order), followed by, in the order of modules, labels, stop points and options
    /// as written, plain lines, quote lines and option texts with stable IDs, and stop command lines whose translatable
    /// arguments (such as input prompts) are written as string literals.
    pub(crate) fn text_entries(&self) -> Vec<(String, &[TextPart])> {
        let mut out = Vec::new();
        for d in &self.identities {
            out.push((
                identity_key(&self.modules[d.module as usize], &d.name),
                d.parts.as_slice(),
            ));
        }
        for l in &self.labels {
            let path = format!("{}.{}", self.modules[l.module as usize], l.name);
            for st in &l.stops {
                if let Some(choice) = &st.choice {
                    for o in &choice.options {
                        if let Some(id) = &o.id {
                            out.push((format!("{path}#{id}"), o.text.as_slice()));
                        }
                    }
                } else if (st.kind != StopKind::Command || st.text_arg.is_some())
                    && let Some(id) = &st.id
                {
                    out.push((format!("{path}#{id}"), st.text.as_slice()));
                }
            }
        }
        out
    }

    /// Finds the stop point of a command with translatable arguments from one history entry (`module.label` of the label and
    /// the stable ID of the stop point).
    pub(crate) fn command_text_arg(&self, label: &str, id: &str) -> Option<TextArg> {
        let (module, name) = label.split_once('.')?;
        let l = self
            .labels
            .iter()
            .find(|l| l.name == name && self.modules[l.module as usize] == module)?;
        l.stops
            .iter()
            .find(|st| st.kind == StopKind::Command && st.id.as_deref() == Some(id))
            .and_then(|st| st.text_arg)
    }

    /// The text table key for the `Identity` of a quote line (the declared name and the original display name). `module` is
    /// the name of the module containing the quote line. When multiple modules have declarations with the same name and
    /// display name, the one in `module` comes first.
    pub(crate) fn identity_key_of(&self, module: &str, id: &str, display: &str) -> Option<String> {
        let mut found = self
            .identities
            .iter()
            .filter(|d| d.name == id && d.display == display)
            .map(|d| self.modules[d.module as usize].as_str());
        let first = found.next()?;
        let m = if first == module || found.any(|m| m == module) {
            module
        } else {
            first
        };
        Some(identity_key(m, id))
    }

    /// The name of the first module (for a program with one module, that module).
    pub fn module(&self) -> &str {
        self.modules.first().map_or("", String::as_str)
    }

    /// The list of module names (in the order passed to compilation).
    pub fn module_names(&self) -> impl Iterator<Item = &str> {
        self.modules.iter().map(String::as_str)
    }

    /// The list of label names of the first module (in declaration order).
    pub fn label_names(&self) -> impl Iterator<Item = &str> {
        self.labels
            .iter()
            .filter(|l| l.module == 0)
            .map(|l| l.name.as_str())
    }

    /// The list of label names of the given module (in declaration order). Empty if the module does not exist.
    pub fn labels_of(&self, module: &str) -> Vec<&str> {
        let Some(idx) = self.modules.iter().position(|m| m == module) else {
            return Vec::new();
        };
        self.labels
            .iter()
            .filter(|l| l.module as usize == idx)
            .map(|l| l.name.as_str())
            .collect()
    }
}

/// The text table key for the display name of an identity (`identity:module.name`; line keys contain `#`, so they do not
/// collide).
pub(crate) fn identity_key(module: &str, name: &str) -> String {
    format!("identity:{module}.{name}")
}

/// FNV-1a (64 bits). Gives the same value on every platform.
pub(crate) fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in *p {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
