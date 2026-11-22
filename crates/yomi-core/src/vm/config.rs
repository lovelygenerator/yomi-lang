//! VM settings.

use super::InterruptHandle;

/// The default of `VmConfig::seed` (fixed; the same seed gives the same random sequence).
pub const DEFAULT_SEED: u64 = 0x5EED_0000_0000_0001;
/// The default of `VmConfig::max_total_bytes` (64 MiB).
pub const DEFAULT_MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;
/// The default of `VmConfig::history_limit`.
pub const DEFAULT_HISTORY_LIMIT: usize = 200;

/// How a load treats variables whose saved type no longer matches the scripts (`spec/host/04-save.md`, 4.6).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoadPolicy {
    /// Fails with a `LoadError` (the default).
    #[default]
    Strict,
    /// Resets such variables to their declared initial values and goes back to the start of the label.
    Lenient,
}

/// VM settings.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// The number of instructions one `run` can execute. `None` means no limit.
    pub max_steps_per_run: Option<u64>,
    /// The maximum depth of calls in scripts. Label `call`s (call stack frames) and `fn` calls (including recursion) are
    /// counted together. Trying to go beyond this is a runtime error (`CallDepthExceeded`).
    pub max_call_depth: u32,
    /// The label to start from. `module.label` (for a program with one module, just the label name also works).
    pub start: String,
    /// The maximum size of one value (bytes). For strings, the length; for `list`, `struct`, `enum` and `Option`, an estimate
    /// of the total of the contents ([`crate::value::Value::approx_bytes`]). Exceeding it is the runtime error
    /// `ValueTooLarge`.
    pub max_value_bytes: usize,
    /// The maximum total size (bytes; the estimate of [`crate::value::Value::approx_bytes`]) of values held in variables.
    /// Counts module variables, host-declared variables and local variables (including call stack frames and `fn` call
    /// frames). Exceeding it is the runtime error `MemoryLimitExceeded`. `None` means no limit. The default is 64 MiB.
    pub max_total_bytes: Option<usize>,
    /// Passed stop points (`module.label#id`). Read by `seen`. Also used when rebuilding the arguments of a stop point on load,
    /// so it is part of the settings passed to `Vm::load`. To replace it later, use [`crate::vm::Vm::set_seen_points`].
    pub seen_points: Vec<String>,
    /// The maximum number of entries kept in the history of stop points ([`crate::vm::Vm::history`]). The oldest are dropped.
    /// 0 means nothing is recorded. The default is 200.
    pub history_limit: usize,
    /// The seed of the random generator (`random`). The same seed, script and input give the same sequence. On load, the state
    /// of the random generator in the save is used (the seed is not).
    pub seed: u64,
    /// Persistent data (the bytes returned by [`crate::vm::Vm::save_persistent`]). If present, the values of
    /// `persistent var`s and the passed stop points are put in when creating (loading) the VM. Takes priority over
    /// `seen_points` (passed stop points are replaced by this). When rebuilding the arguments of a stop point on load,
    /// scripts that read `persistent var`s and `seen` get the same arguments as when saved. To pass it later, use
    /// [`crate::vm::Vm::load_persistent`].
    pub persistent: Option<Vec<u8>>,
    /// How loads treat variables whose type no longer matches the scripts. Applies to `Vm::load`, `Vm::new` with
    /// `persistent` and `Vm::load_persistent`. The default is [`LoadPolicy::Strict`].
    pub load_policy: LoadPolicy,
    /// The token that interrupts a `run` from another thread (`spec/host/01-execution.md`, 1.9a). If `None`, the VM makes its
    /// own (get it with [`crate::vm::Vm::interrupt_handle`]). Pass one to share it between VMs.
    pub interrupt: Option<InterruptHandle>,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            max_steps_per_run: Some(50_000_000),
            max_call_depth: 256,
            start: "start".to_string(),
            max_value_bytes: 16 * 1024 * 1024,
            max_total_bytes: Some(DEFAULT_MAX_TOTAL_BYTES),
            seen_points: Vec::new(),
            history_limit: DEFAULT_HISTORY_LIMIT,
            seed: DEFAULT_SEED,
            persistent: None,
            load_policy: LoadPolicy::Strict,
            interrupt: None,
        }
    }
}

impl VmConfig {
    /// Sets how loads treat variables whose type no longer matches the scripts.
    pub fn load_policy(mut self, policy: LoadPolicy) -> Self {
        self.load_policy = policy;
        self
    }

    /// Sets the maximum size of one value (bytes).
    pub fn max_value_bytes(mut self, bytes: usize) -> Self {
        self.max_value_bytes = bytes;
        self
    }

    /// Sets the maximum total size of values held in variables (bytes; `None` means no limit).
    pub fn max_total_bytes(mut self, bytes: Option<usize>) -> Self {
        self.max_total_bytes = bytes;
        self
    }

    /// Sets the label to start from (`module.label`; with one module, just the label name also works).
    pub fn start(mut self, label: &str) -> Self {
        self.start = label.to_string();
        self
    }

    /// Sets the passed stop points (`module.label#id`; as returned by [`crate::vm::Vm::seen_points`]).
    pub fn seen_points<I, S>(mut self, points: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.seen_points = points.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the maximum number of entries in the history of stop points (0 means nothing is recorded).
    pub fn history_limit(mut self, limit: usize) -> Self {
        self.history_limit = limit;
        self
    }

    /// Sets the seed of the random generator.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Sets the persistent data (the bytes of [`crate::vm::Vm::save_persistent`]).
    pub fn persistent_data(mut self, bytes: Vec<u8>) -> Self {
        self.persistent = Some(bytes);
        self
    }

    /// Sets the maximum depth of calls.
    pub fn max_call_depth(mut self, depth: u32) -> Self {
        self.max_call_depth = depth;
        self
    }

    /// Sets the token that interrupts a `run`.
    pub fn interrupt(mut self, handle: InterruptHandle) -> Self {
        self.interrupt = Some(handle);
        self
    }

    /// Sets the maximum number of instructions in one `run`.
    pub fn max_steps_per_run(mut self, steps: Option<u64>) -> Self {
        self.max_steps_per_run = steps;
        self
    }
}
