//! The types of command functions, and conversion of return values.

use super::*;

/// The return value of a command function (`T` or `Result<T, CommandError>`).
pub trait IntoReturn {
    /// The return type as seen from scripts.
    fn ret_ty() -> Ty;
    /// Makes a value.
    fn into_return(self) -> Result<Value, CommandError>;
}

impl<T: IntoValue> IntoReturn for T {
    fn ret_ty() -> Ty {
        T::ty()
    }
    fn into_return(self) -> Result<Value, CommandError> {
        Ok(self.into_value())
    }
}

impl<T: IntoValue> IntoReturn for Result<T, CommandError> {
    fn ret_ty() -> Ty {
        T::ty()
    }
    fn into_return(self) -> Result<Value, CommandError> {
        self.map(IntoValue::into_value)
    }
}

/// The error when the number or types of arguments do not match the registration (the code is `core.bad_args`).
pub(super) fn bad_args(name: &str) -> CommandError {
    CommandError::with_code("core.bad_args", [("name", name.into())])
}

/// The function of an effect command (`Fn(&mut Ctx<P>, args...) -> R`; 0 to 6 arguments).
pub trait EffectFn<P: Package, Args, R> {
    /// The argument types.
    fn arg_types() -> Vec<Ty>;
    /// The return type.
    fn ret_ty() -> Ty;
    /// Calls it.
    fn call(&self, ctx: &mut Ctx<'_, P>, args: &[Value]) -> Result<Value, CommandError>;
}

/// The function of a query command (`Fn(&Ctx<P>, args...) -> R`; 0 to 6 arguments).
pub trait QueryFn<P: Package, Args, R> {
    /// The argument types.
    fn arg_types() -> Vec<Ty>;
    /// The return type.
    fn ret_ty() -> Ty;
    /// Calls it.
    fn call(&self, ctx: &Ctx<'_, P>, args: &[Value]) -> Result<Value, CommandError>;
}

/// A macro that implements `EffectFn`, `QueryFn` and others for the types of command functions (0 to 6 arguments).
macro_rules! impl_fns {
    ($n:expr; $($A:ident $a:ident $i:tt),*) => {
        impl<P: Package, F, R, $($A),*> EffectFn<P, ($($A,)*), R> for F
        where
            F: Fn(&mut Ctx<'_, P>, $($A),*) -> R,
            R: IntoReturn,
            $($A: FromValue,)*
        {
            fn arg_types() -> Vec<Ty> {
                vec![$($A::ty()),*]
            }
            fn ret_ty() -> Ty {
                R::ret_ty()
            }
            fn call(&self, ctx: &mut Ctx<'_, P>, args: &[Value]) -> Result<Value, CommandError> {
                if args.len() != $n {
                    return Err(bad_args(P::NAME));
                }
                $(let $a = $A::from_value(&args[$i]).ok_or_else(|| bad_args(P::NAME))?;)*
                self(ctx, $($a),*).into_return()
            }
        }

        impl<P: Package, F, R, $($A),*> QueryFn<P, ($($A,)*), R> for F
        where
            F: Fn(&Ctx<'_, P>, $($A),*) -> R,
            R: IntoReturn,
            $($A: FromValue,)*
        {
            fn arg_types() -> Vec<Ty> {
                vec![$($A::ty()),*]
            }
            fn ret_ty() -> Ty {
                R::ret_ty()
            }
            fn call(&self, ctx: &Ctx<'_, P>, args: &[Value]) -> Result<Value, CommandError> {
                if args.len() != $n {
                    return Err(bad_args(P::NAME));
                }
                $(let $a = $A::from_value(&args[$i]).ok_or_else(|| bad_args(P::NAME))?;)*
                self(ctx, $($a),*).into_return()
            }
        }
    };
}

impl_fns!(0;);
impl_fns!(1; A1 a1 0);
impl_fns!(2; A1 a1 0, A2 a2 1);
impl_fns!(3; A1 a1 0, A2 a2 1, A3 a3 2);
impl_fns!(4; A1 a1 0, A2 a2 1, A3 a3 2, A4 a4 3);
impl_fns!(5; A1 a1 0, A2 a2 1, A3 a3 2, A4 a4 3, A5 a5 4);
impl_fns!(6; A1 a1 0, A2 a2 1, A3 a3 2, A4 a4 3, A5 a5 4, A6 a6 5);
