//! Backend: turns the IR into code that the VM runs.
//!
//! A backend implements [`Backend`]. Every backend keeps the following (so that the results of execution do not depend on
//! the backend).
//!
//! - The order of evaluation follows the order of the IR (side-effecting expressions are not reordered).
//! - For each stop point, the start of the range that computes the arguments and the position of the stopping instruction
//!   are recorded in [`StopInfo`](crate::program::StopInfo). Running that range again gives the same arguments. The
//!   position of a `call` is recorded in [`CallSite`](crate::program::CallSite).
//! - The size of a value is counted when it is stored in a variable (temporary values of expressions are not counted).
//! - Each instruction has a line number, and the line of a runtime error is the same regardless of the backend.
//! - Backends are not mixed within one program.

use crate::ir::Body;

pub(crate) mod register;

/// Turns an IR body (a label or `fn`) into code that the VM runs.
pub(crate) trait Backend {
    /// The code of one body.
    type Code;
    /// The reason the code could not be made (such as exceeding a limit only the backend knows). The IR is already checked,
    /// so it fails for no other reason.
    type Error;

    fn lower(&mut self, body: &Body) -> Result<Self::Code, Self::Error>;
}
