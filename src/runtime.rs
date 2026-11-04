//! Local native execution. The converter's `run` command uses a worker process.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Output {
    pub kind: i32,
    /// Raw bytes, preserving binary output and the original callback type.
    pub data: Vec<u8>,
}

/// Execute a local object and collect synchronous Beacon output.
///
/// # Safety
/// The caller must trust the object's native code and its `void(char*, int)`
/// entrypoint. It must return normally and leave no threads, callbacks, or pointers
/// referring to the mapped object. Native faults cannot be caught as Rust errors.
/// Validated AMD64 function tables are registered for the call's lifetime. This
/// function does not sandbox code or initialize a C++ runtime, TLS, or constructors.
pub unsafe fn execute(
    bytes: &[u8],
    entry: &str,
    arguments: &mut [u8],
) -> anyhow::Result<Vec<Output>> {
    #[cfg(all(windows, target_arch = "x86_64"))]
    // SAFETY: the public contract is forwarded to the platform runtime.
    unsafe {
        windows::execute(bytes, entry, arguments)
    }
    #[cfg(not(all(windows, target_arch = "x86_64")))]
    {
        let _ = (bytes, entry, arguments);
        anyhow::bail!("execution requires Windows x64");
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;
