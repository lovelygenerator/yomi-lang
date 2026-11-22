//! The `flow` package: waiting in the flow (stop commands only; `spec/packages/flow.md`).

use yomi_core::{CommandError, Package, PackageReg};

/// The `flow` package. It has neither a state slice nor a persistent slice.
///
/// - `flow.wait(seconds: float)`: stops for `seconds` seconds. A negative value is a runtime error (it does not stop).
/// - `flow.wait_input()`: stops until there is input.
///
/// Measuring time and receiving input are the host's job.
pub struct Flow;

impl Package for Flow {
    const NAME: &'static str = "flow";
    const STD: bool = true;
    type State = ();
    type Persistent = ();

    fn register(reg: &mut PackageReg<'_, Self>) {
        reg.stop_checked::<(f64,), (), _>("flow.wait", |(seconds,)| {
            if seconds < 0.0 {
                Err(CommandError::with_code(
                    "flow.negative_seconds",
                    [("seconds", seconds.to_string().into())],
                ))
            } else {
                Ok(())
            }
        })
        .doc("seconds 秒、止まる（負の秒数は実行時エラー）")
        .param_names(&["seconds"]);
        reg.stop::<(), ()>("flow.wait_input")
            .doc("何かの入力があるまで止まる")
            .param_names(&[]);
    }
}
