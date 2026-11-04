//! Small executable template used by `bof2exe build`.
fn main() {
    let result = (|| -> anyhow::Result<()> {
        let executable = std::env::current_exe()?;
        let payload =
            bof2exe::bundle::read(&mut std::fs::File::open(executable)?)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "This is a build template. Use `bof2exe build` to create a command-line tool."
                )
            })?;
        // SAFETY: invocation opts into the program embedded by an explicit build operation.
        unsafe { bof2exe::standalone::run_embedded(payload) }
    })();
    if let Err(error) = result {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
