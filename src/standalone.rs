//! Entry-point support shared by the converter and its smaller runner template.
use crate::{
    args::{pack, Argument},
    bundle::Payload,
    runtime,
};
use anyhow::{ensure, Result};
use std::io::Write;

/// Run an explicitly invoked executable's embedded program.
///
/// # Safety
/// The embedded object must satisfy `runtime::execute`'s native-code contract.
pub unsafe fn run_embedded(payload: Payload) -> Result<()> {
    let (replacement, error_exit) = if let Some(definition) = &payload.tool {
        let values = match definition.parse_from(std::env::args_os()) {
            Ok(values) => values,
            Err(error) => error.exit(),
        };
        (Some(pack(&values)?), definition.error_exit)
    } else {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.first().is_some_and(|v| v == "--help" || v == "-h") {
            println!("Embedded COFF runner\n\nUsage: runner.exe [-- TYPE:VALUE ...]\n\nNo arguments uses embedded defaults. Types: str, wstr, int, short, bin, hex.");
            return Ok(());
        }
        let arguments = if !args.is_empty() {
            ensure!(
                args[0] == "--",
                "expected -- before replacement typed arguments"
            );
            Some(pack(
                &args[1..]
                    .iter()
                    .map(|v| v.parse())
                    .collect::<Result<Vec<Argument>, _>>()?,
            )?)
        } else {
            None
        };
        (arguments, false)
    };
    let (object, stored) = payload.decode()?;
    let mut arguments = replacement.unwrap_or(stored);
    // SAFETY: the executable caller explicitly opts into this embedded native program.
    let output = unsafe { runtime::execute(&object, &payload.entry, &mut arguments)? };
    let failed = error_exit && output.iter().any(|o| o.kind == 13);
    for item in output {
        if item.kind == 13 {
            std::io::stderr().write_all(&item.data)?;
        } else {
            std::io::stdout().write_all(&item.data)?;
        }
    }
    std::io::stdout().flush()?;
    std::io::stderr().flush()?;
    if failed {
        std::process::exit(1);
    }
    Ok(())
}
