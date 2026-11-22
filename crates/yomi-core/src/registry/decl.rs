//! Types of registry declarations (commands, variables, package entries, sugar).

use super::*;

/// The string type (`str` in scripts).
pub type Str = String;

/// The ID of a command (in registration order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandId(pub u32);

/// The kind of sugar.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sugar {
    /// A plain line.
    Plain,
    /// A quote line.
    Quote,
    /// A choice.
    Choice,
}

impl Sugar {
    /// The name used in diagnostic arguments.
    pub fn name(self) -> &'static str {
        match self {
            Sugar::Plain => "plain",
            Sugar::Quote => "quote",
            Sugar::Choice => "choice",
        }
    }
}

/// A sugar binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Binding {
    /// Bound to a stop command.
    Command(CommandId),
    /// Ignored (only checked).
    Ignored,
}

/// The kind of a command (section 2.3 of `spec/host/02-registry.md`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Effect. The VM calls the function. Changes its own package's state. Does not stop.
    Effect,
    /// Query. The VM calls the function. Only reads state and returns a value.
    Query,
    /// Stop. The VM stops and the host does the work.
    Stop,
}

impl CommandKind {
    /// The name used in diagnostic arguments.
    pub fn name(self) -> &'static str {
        match self {
            CommandKind::Effect => "effect",
            CommandKind::Query => "query",
            CommandKind::Stop => "stop",
        }
    }
}

/// The declaration of a stop command.
#[derive(Debug, Clone, PartialEq)]
pub struct StopDecl {
    /// The command name (`package.command`).
    pub name: String,
    /// Parameter types.
    pub args: Vec<Ty>,
    /// The return type.
    pub ret: Ty,
    /// The package that provides the command (only when registered with `Registry::package`). Scripts cannot use this command
    /// unless they `import` that package.
    pub package: Option<String>,
    /// Parameter names (only those given at registration; as many as the parameters).
    pub params: Vec<Option<String>>,
    /// Default values of parameters (as many as the parameters; parameters with defaults come last).
    pub defaults: Vec<Option<Value>>,
    /// A description (used for completion in tools).
    pub doc: Option<String>,
    /// The indices of parameters (`str`) that a text table can translate. Only when the script writes them as string literals
    /// and the line has a stable ID, they are replaced with the translation under that line's key (`module.label#id`)
    /// (section 5.2a of `spec/host/05-texts.md`).
    pub text_param: Option<usize>,
}

/// The kind of a variable (section 2.7 of `spec/host/02-registry.md`).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A variable in saves (same as `save var` in scripts).
    Save,
    /// A variable in persistent data (same as `persistent var` in scripts).
    Persistent,
}

/// A variable declared by the host.
#[derive(Debug, Clone, PartialEq)]
pub struct VarDecl {
    /// The name (`package.variable`).
    pub name: String,
    /// The type (`int`, `float`, `bool`, `str`).
    pub ty: Ty,
    /// Kind.
    pub kind: Kind,
    /// The initial value.
    pub init: Value,
    /// Whether scripts cannot assign to it.
    pub read_only: bool,
    /// A description (used for completion in tools).
    pub doc: Option<String>,
}

/// The continuation of registering a variable (to add `.read_only()`).
#[derive(Debug)]
pub struct VarHandle<'r> {
    pub(super) var: Option<&'r mut VarDecl>,
}

impl VarHandle<'_> {
    /// Makes the variable one that scripts cannot assign to (a compile error; the host can still write it with `set_var`).
    pub fn read_only(mut self) -> Self {
        if let Some(v) = self.var.as_deref_mut() {
            v.read_only = true;
        }
        self
    }

    /// Adds a description to the variable (written to declaration files).
    pub fn doc(mut self, text: &str) -> Self {
        if let Some(v) = self.var.as_deref_mut() {
            v.doc = Some(text.to_string());
        }
        self
    }
}

/// The kind of effect / query (internal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeKind {
    Effect,
    Query,
}

/// A registered function (type-erased).
pub(crate) type NativeFn =
    Arc<dyn Fn(Env<'_>, &[Value]) -> Result<Value, CommandError> + Send + Sync>;

/// An effect or query command.
#[derive(Clone)]
pub(crate) struct NativeDecl {
    pub name: String,
    pub kind: NativeKind,
    pub args: Vec<Ty>,
    pub ret: Ty,
    /// An index into `Registry::packages`.
    pub package: usize,
    pub func: NativeFn,
    pub params: Vec<Option<String>>,
    pub defaults: Vec<Option<Value>>,
    pub doc: Option<String>,
}

impl std::fmt::Debug for NativeDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeDecl")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// Validation of the arguments of a stop command (called before the VM returns the stop notification).
#[derive(Clone)]
pub(crate) struct StopCheck(Arc<StopCheckFn>);

/// A function that validates the arguments of a stop command.
type StopCheckFn = dyn Fn(&[Value]) -> Result<(), CommandError> + Send + Sync;

impl StopCheck {
    /// Makes one from a checking function.
    pub(crate) fn new(
        f: impl Fn(&[Value]) -> Result<(), CommandError> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(f))
    }

    /// Checks the arguments.
    pub(crate) fn run(&self, args: &[Value]) -> Result<(), CommandError> {
        (self.0)(args)
    }
}

impl std::fmt::Debug for StopCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StopCheck(..)")
    }
}

/// Validation of the return value of a stop command (called by `resume` before passing the value received from the host to
/// the script).
#[derive(Clone)]
pub(crate) struct RetCheck(Arc<RetCheckFn>);

/// A function that validates the return value of a stop command (the arguments at the stop, and the value returned).
type RetCheckFn = dyn Fn(&[Value], &Value) -> Result<(), CommandError> + Send + Sync;

impl RetCheck {
    /// Makes one from a checking function.
    pub(crate) fn new(
        f: impl Fn(&[Value], &Value) -> Result<(), CommandError> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(f))
    }

    /// Checks the arguments and the return value given by the host.
    pub(crate) fn run(&self, args: &[Value], ret: &Value) -> Result<(), CommandError> {
        (self.0)(args, ret)
    }
}

impl std::fmt::Debug for RetCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RetCheck(..)")
    }
}

/// The result of looking up a command by name (internal).
#[derive(Debug, Clone)]
pub(crate) struct CommandInfo {
    /// The official name (when looked up by an alias, the name the alias points to).
    pub name: String,
    /// Whether it was found by an alias (use of an old name; subject to a warning).
    pub via_alias: bool,
    pub kind: CommandKind,
    pub args: Vec<Ty>,
    pub ret: Ty,
    /// The namespace that scripts must `import` (only for package commands; when looked up by an alias, the alias's
    /// namespace).
    pub package: Option<String>,
    /// Parameter names (`None` if not given).
    pub params: Vec<Option<String>>,
    /// Default values of parameters.
    pub defaults: Vec<Option<Value>>,
}

/// The declaration of a command (of any kind; for tools and declaration files; [`Registry::commands`]).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandDecl {
    /// The name (`package.command`).
    pub name: String,
    /// Kind.
    pub kind: CommandKind,
    /// Parameter types.
    pub args: Vec<Ty>,
    /// The return type (`()` if none).
    pub ret: Ty,
    /// Parameter names (only those given at registration).
    pub params: Vec<Option<String>>,
    /// Old names (aliases), in name order.
    pub aliases: Vec<String>,
    /// A description.
    pub doc: Option<String>,
    /// The package that provides the command.
    pub package: Option<String>,
    /// Default values of parameters (as many as the parameters).
    pub defaults: Vec<Option<Value>>,
}

/// Information about a registered package (for declaration files and tools).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageDecl {
    /// The name.
    pub name: String,
    /// The version of the state slice.
    pub version: u32,
    /// The version of the persistent slice.
    pub persistent_version: u32,
    /// The packages whose state this reads.
    pub depends: Vec<String>,
}

/// A type-erased package entry passed to bulk registration ([`Registry::package_set`]). Made with
/// [`packages!`](crate::packages).
#[derive(Clone, Copy)]
pub struct PackageItem {
    pub(super) name: &'static str,
    pub(super) depends: &'static [&'static str],
    pub(super) stage: fn(&mut Registry) -> Result<(), RegistryError>,
}

impl PackageItem {
    /// The entry for package `P`.
    pub fn of<P: Package>() -> Self {
        Self {
            name: P::NAME,
            depends: P::DEPENDS,
            stage: |reg| {
                reg.packages.push(PackageEntry::of::<P>());
                let index = reg.packages.len() - 1;
                let mut pr = PackageReg::<P>::new(reg, index);
                P::register(&mut pr);
                pr.finish()
            },
        }
    }

    /// The name of the package.
    pub fn name(&self) -> &'static str {
        self.name
    }
}

impl std::fmt::Debug for PackageItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackageItem({})", self.name)
    }
}

/// Turns multiple packages into a list passed to [`Registry::package_set`].
///
/// ```
/// # use yomi_core::{packages, Registry, Package, PackageReg};
/// # struct A; struct B;
/// # impl Package for A { const NAME: &'static str = "a"; const DEPENDS: &'static [&'static str] = &["b"];
/// #   type State = (); type Persistent = (); fn register(_: &mut PackageReg<'_, Self>) {} }
/// # impl Package for B { const NAME: &'static str = "b";
/// #   type State = (); type Persistent = (); fn register(_: &mut PackageReg<'_, Self>) {} }
/// let mut reg = Registry::new();
/// reg.package_set(&packages![A, B]).unwrap(); // A depends on B, but they are reordered
/// ```
#[macro_export]
macro_rules! packages {
    ($($p:ty),* $(,)?) => {
        [$($crate::PackageItem::of::<$p>()),*]
    };
}
