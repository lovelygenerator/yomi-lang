//! Registering a package's commands and variables.

use super::*;

/// A typed registration entry passed to [`Package::register`].
///
/// Registration failures (the first half of a name is not `P::NAME`, double registration of the same name, and so on) are
/// remembered (the first one), and [`Registry::package`] returns it as `Err` (the registry is then unchanged).
pub struct PackageReg<'r, P: Package> {
    pub(super) reg: &'r mut Registry,
    pub(super) package: usize,
    pub(super) error: Option<RegistryError>,
    /// The name of the command, variable or type registered just before (what `alias`, `doc` and `param_names` attach to).
    pub(super) last: Option<String>,
    /// Whether what was registered just before is a command.
    pub(super) last_is_command: bool,
    pub(super) _package: PhantomData<fn() -> P>,
}

impl<'r, P: Package> PackageReg<'r, P> {
    /// Makes the registration entry of a package. `package` is the position of the package.
    pub(crate) fn new(reg: &'r mut Registry, package: usize) -> Self {
        Self {
            reg,
            package,
            error: None,
            last: None,
            last_is_command: false,
            _package: PhantomData,
        }
    }

    /// Finishes registration. If there was an error along the way, returns the first error.
    pub(crate) fn finish(self) -> Result<(), RegistryError> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Records an error (keeps only the first one).
    fn fail(&mut self, e: RegistryError) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }

    /// Calls `f` for the command registered just before. `NoCommand` if the previous one is not a command.
    fn with_last_command(
        &mut self,
        f: impl FnOnce(&mut Registry, &str) -> Result<(), RegistryError>,
    ) -> &mut Self {
        let result = match self.last.clone() {
            Some(name) if self.last_is_command => f(self.reg, &name),
            _ => Err(RegistryError::NoCommand),
        };
        if let Err(e) = result {
            self.fail(e);
        }
        self
    }

    /// Receives the result of registering a command (on success, it becomes what `alias` and `doc` attach to).
    fn done_command<T>(&mut self, name: &str, result: Result<T, RegistryError>) -> &mut Self {
        match result {
            Ok(_) => {
                self.last = Some(name.to_string());
                self.last_is_command = true;
            }
            Err(e) => self.fail(e),
        }
        self
    }

    /// Registers an old name (alias) of the command registered just before (section 2.2 of `spec/host/02-registry.md`). The
    /// old name is `namespace.name`, and may be in another package's namespace.
    pub fn alias(&mut self, old: &str) -> &mut Self {
        self.with_last_command(|reg, name| reg.alias(old, name))
    }

    /// Adds a description to the command, variable or type registered just before.
    pub fn doc(&mut self, text: &str) -> &mut Self {
        match self.last.clone() {
            Some(name) => {
                if let Err(e) = self.reg.doc(&name, text) {
                    self.fail(e);
                }
            }
            None => self.fail(RegistryError::NoCommand),
        }
        self
    }

    /// Names the parameters of the command registered just before. The count must be the same as the parameters.
    pub fn param_names(&mut self, names: &[&str]) -> &mut Self {
        self.with_last_command(|reg, name| reg.param_names(name, names))
    }

    /// Adds default values to the parameters of the command registered just before (names are those given with
    /// `param_names`; [`Registry::param_defaults`]).
    pub fn defaults(&mut self, defaults: &[(&str, Value)]) -> &mut Self {
        self.with_last_command(|reg, name| reg.param_defaults(name, defaults))
    }

    /// Makes parameter `index` (`str`) of the stop command registered just before a body that text tables can translate (such
    /// as input prompts). It is replaced with that line's key only when the script writes it as a string literal and the line
    /// has a stable ID.
    pub fn text_param(&mut self, index: usize) -> &mut Self {
        self.with_last_command(|reg, name| reg.text_param(name, index))
    }

    /// Registers a `struct` or `enum` (a type with `#[derive(YomiType)]`). The namespace must be the package's name
    /// (`InvalidName` otherwise). Register the types it uses first.
    pub fn ty<T: crate::registry::YomiDecl>(&mut self) -> &mut Self {
        let decl = T::decl();
        let name = decl.name.clone();
        match self.reg.add_type(Some(P::NAME), decl) {
            Ok(()) => {
                self.last = Some(name);
                self.last_is_command = false;
            }
            Err(e) => self.fail(e),
        }
        self
    }

    /// Registers an effect command. The function is `Fn(&mut Ctx<P>, args...) -> R` (`R` is `()`, a type usable in scripts,
    /// or `Result<that, CommandError>`). Parameter and return types are derived from the function. The function is
    /// `Send + Sync` (so the VM can run on other threads).
    pub fn effect<Args, R, F: EffectFn<P, Args, R> + Send + Sync + 'static>(
        &mut self,
        name: &str,
        f: F,
    ) -> &mut Self {
        let result = self.reg.add_native(
            self.package,
            name,
            crate::registry::NativeKind::Effect,
            F::arg_types(),
            F::ret_ty(),
            std::sync::Arc::new(move |env: Env<'_>, args: &[Value]| {
                let mut ctx = Ctx::<P>::new(env)?;
                f.call(&mut ctx, args)
            }),
        );
        self.done_command(name, result)
    }

    /// Registers a query command. The function is `Fn(&Ctx<P>, args...) -> R`. It does not change state and returns the same
    /// value for the same state. The return value cannot be `()`. The function is `Send + Sync`.
    pub fn query<Args, R, F: QueryFn<P, Args, R> + Send + Sync + 'static>(
        &mut self,
        name: &str,
        f: F,
    ) -> &mut Self {
        let result = self.reg.add_native(
            self.package,
            name,
            crate::registry::NativeKind::Query,
            F::arg_types(),
            F::ret_ty(),
            std::sync::Arc::new(move |env: Env<'_>, args: &[Value]| {
                let ctx = Ctx::<P>::new(env)?;
                f.call(&ctx, args)
            }),
        );
        self.done_command(name, result)
    }

    /// Registers a stop command by declaring only its types (the host does the work).
    pub fn stop<A: YomiArgs, R: YomiType>(&mut self, name: &str) -> &mut Self {
        let result = self
            .reg
            .add_stop(name, A::types(), R::ty(), Some(P::NAME), None, None);
        self.done_command(name, result)
    }

    /// Registers a stop command with validation. `check` is called with the evaluated arguments before the VM returns the stop
    /// notification. If it is `Err`, it does not stop and becomes a runtime error (the VM adds the position). `check` is
    /// `Send + Sync`.
    pub fn stop_checked<A, R, F>(&mut self, name: &str, check: F) -> &mut Self
    where
        A: FromArgs + 'static,
        R: YomiType,
        F: Fn(A) -> Result<(), CommandError> + Send + Sync + 'static,
    {
        let verify =
            crate::registry::StopCheck::new(move |args: &[Value]| match A::from_values(args) {
                Some(a) => check(a),
                None => Err(bad_args("stop")),
            });
        let result =
            self.reg
                .add_stop(name, A::types(), R::ty(), Some(P::NAME), Some(verify), None);
        self.done_command(name, result)
    }

    /// Registers a stop command with checks of arguments and return values.
    ///
    /// - `check_args` is the same as `check` of `stop_checked` (called with the evaluated arguments before stopping; if `Err`,
    ///   it does not stop and becomes a runtime error).
    /// - `check_return` is called with the arguments at the stop and the returned value, after the type of the value the host
    ///   returned with `Input::Return` has matched and before the value is passed to the script. If `Err`, `resume` returns a
    ///   runtime error (the VM adds the position; the VM is left in the state after a runtime error and can be restored from a
    ///   snapshot with `restore`).
    /// - Both functions are `Send + Sync`.
    pub fn stop_validated<A, R, F, G>(
        &mut self,
        name: &str,
        check_args: F,
        check_return: G,
    ) -> &mut Self
    where
        A: FromArgs + 'static,
        R: FromValue + 'static,
        F: Fn(&A) -> Result<(), CommandError> + Send + Sync + 'static,
        G: Fn(&A, &R) -> Result<(), CommandError> + Send + Sync + 'static,
    {
        let verify =
            crate::registry::StopCheck::new(move |args: &[Value]| match A::from_values(args) {
                Some(a) => check_args(&a),
                None => Err(bad_args("stop")),
            });
        let verify_return =
            crate::registry::RetCheck::new(move |args: &[Value], ret: &Value| {
                match (A::from_values(args), R::from_value(ret)) {
                    (Some(a), Some(r)) => check_return(&a, &r),
                    _ => Err(bad_args("stop")),
                }
            });
        let result = self.reg.add_stop(
            name,
            A::types(),
            R::ty(),
            Some(P::NAME),
            Some(verify),
            Some(verify_return),
        );
        self.done_command(name, result)
    }

    /// Registers a variable declared by the host. The name is `P::NAME.variable`. `.read_only()` makes it impossible for
    /// scripts to assign to it. The type is one of `int`, `float`, `bool`, `str`.
    pub fn var<T: YomiType + IntoValue>(
        &mut self,
        name: &str,
        kind: Kind,
        initial: T,
    ) -> VarHandle<'_> {
        match self
            .reg
            .add_var(Some(P::NAME), name, T::ty(), kind, initial.into_value())
        {
            Ok(index) => {
                self.last = Some(name.to_string());
                self.last_is_command = false;
                self.reg.var_handle(Some(index))
            }
            Err(e) => {
                self.fail(e);
                self.reg.var_handle(None)
            }
        }
    }
}
