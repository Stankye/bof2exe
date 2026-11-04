use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "bof2exe",
    version,
    about = "Turn Windows BOFs into standalone command-line tools"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create an editable command-line schema for a BOF, optionally using CNA metadata.
    Init {
        #[command(flatten)]
        source: ToolSource,
        /// New schema path; defaults to the object name with .tool.toml.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Build a standalone CLI from a tool schema or directly from a BOF/CNA signature.
    Build {
        #[command(flatten)]
        source: ToolSource,
        /// New standalone executable path.
        #[arg(short, long)]
        output: PathBuf,
        /// Also write a Markdown reference for the generated command.
        #[arg(long)]
        docs: Option<PathBuf>,
    },
    /// Describe sections, symbols, imports, and compatibility without executing code.
    Inspect {
        #[command(flatten)]
        input: Input,
        /// Emit a machine-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Validate an object; exit unsuccessfully when static compatibility checks fail.
    Check {
        #[command(flatten)]
        input: Input,
        #[arg(long)]
        json: bool,
    },
    /// Run an object in a separate worker process (Windows x64).
    Run {
        #[command(flatten)]
        input: Input,
        /// Deadline in seconds; terminate the worker when it expires.
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..=86400))]
        timeout: u64,
        /// Return callback kinds and raw byte arrays as JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        parameters: Parameters,
        /// Replace manifest defaults with typed arguments after --.
        #[arg(last = true)]
        arguments: Vec<String>,
    },
    /// Create an executable containing the object and default arguments (Windows x64).
    Bundle {
        #[command(flatten)]
        input: Input,
        /// New executable path. Existing files are never overwritten.
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        parameters: Parameters,
        #[arg(last = true)]
        arguments: Vec<String>,
    },
    /// Encode typed arguments for use with the library or another Beacon runtime.
    Pack {
        /// New output file; otherwise emit base64 to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[command(flatten)]
        parameters: Parameters,
        #[arg(last = true)]
        arguments: Vec<String>,
    },
    /// Analyze CNA command registrations, packing signatures, and parameter expressions.
    Cna {
        path: PathBuf,
        #[arg(long, conflicts_with = "docs")]
        json: bool,
        /// Emit Markdown command and parameter documentation.
        #[arg(long)]
        docs: bool,
    },
    /// Combine objects/resources into deterministic relocatable COFF.
    Link {
        #[arg(required_unless_present = "spec", conflicts_with = "spec")]
        objects: Vec<PathBuf>,
        /// Versioned TOML composition spec; paths resolve beside the spec.
        #[arg(long)]
        spec: Option<PathBuf>,
        /// Search a .lib/.a archive or ZIP collection for required COFF members.
        #[arg(short = 'L', long = "library")]
        libraries: Vec<PathBuf>,
        /// Include every supplied library member, instead of resolving on demand.
        #[arg(long)]
        whole_archive: bool,
        /// New COFF output path (required unless --plan is set).
        #[arg(short, long, required_unless_present = "plan")]
        output: Option<PathBuf>,
        /// Inspect the link result without writing it.
        #[arg(long, conflicts_with = "output")]
        plan: bool,
        #[arg(short, long)]
        entry: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Print the implemented compatibility surface and remaining gaps.
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// Inspect definitions and references in a COFF archive or ZIP object collection.
    Library {
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Inspect a local fixture suite; --execute also checks output in isolated workers.
    Suite {
        path: PathBuf,
        #[arg(long)]
        execute: bool,
        #[arg(long)]
        json: bool,
    },
    /// Generate Markdown CLI reference and roff manuals from this command model.
    Docs {
        #[arg(long, default_value = "docs/generated")]
        output: PathBuf,
    },
    /// Generate command and option name completion from this command model.
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Describe or document a versioned module manifest.
    Module {
        manifest: PathBuf,
        /// Emit generated Markdown module documentation instead of TOML metadata.
        #[arg(long)]
        docs: bool,
    },
    #[command(name = "__worker", hide = true)]
    Worker,
}
#[derive(Debug, Args)]
pub struct Input {
    /// Local standard COFF object.
    #[arg(required_unless_present = "manifest", conflicts_with = "manifest")]
    pub object: Option<PathBuf>,
    /// TOML manifest; relative object paths resolve beside this file.
    #[arg(long, conflicts_with = "object")]
    pub manifest: Option<PathBuf>,
    /// Override the manifest entrypoint (otherwise defaults to go).
    #[arg(short, long)]
    pub entry: Option<String>,
}
#[derive(Debug, Args)]
pub struct ToolSource {
    /// Tool schema (.toml), or a local BOF for direct conversion.
    pub input: PathBuf,
    #[command(flatten)]
    pub parameters: Parameters,
    /// Generated command name; defaults to the BOF filename or schema name.
    #[arg(long)]
    pub name: Option<String>,
    /// Description shown in the generated command's help.
    #[arg(long)]
    pub description: Option<String>,
    #[arg(short, long)]
    pub entry: Option<String>,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    PowerShell,
}

#[derive(Debug, Default, Args)]
pub struct Parameters {
    /// Raw values follow this bof_pack format: b, i, s, z, Z.
    #[arg(long, conflicts_with = "cna")]
    pub format: Option<String>,
    /// Infer wire types from a CNA command without executing the script.
    #[arg(long, requires = "command")]
    pub cna: Option<PathBuf>,
    /// Alias/subroutine containing the selected bof_pack call.
    #[arg(long, requires = "cna")]
    pub command: Option<String>,
    /// One-based bof_pack call index for commands with multiple pack calls.
    #[arg(long, requires = "cna")]
    pub variant: Option<usize>,
}
impl Parameters {
    pub fn resolve(
        &self,
        values: &[String],
        defaults: &[bof2exe::args::Argument],
    ) -> anyhow::Result<Vec<bof2exe::args::Argument>> {
        if let Some(format) = &self.format {
            return bof2exe::args::from_format(format, values);
        }
        if let Some(path) = &self.cna {
            let report = bof2exe::cna::analyze_file(path)?;
            let format =
                report.signature(self.command.as_deref().unwrap_or_default(), self.variant)?;
            eprintln!("Using CNA wire signature {format:?}; supplied values follow bof_pack order. Script transformations and defaults are not evaluated.");
            return bof2exe::args::from_format(format, values);
        }
        if values.is_empty() {
            Ok(defaults.to_vec())
        } else {
            values.iter().map(|v| Ok(v.parse()?)).collect()
        }
    }
    pub fn input_entry(&self, mut input: Input) -> anyhow::Result<Input> {
        if input.entry.is_none() && input.manifest.is_none() {
            if let Some(path) = &self.cna {
                let report = bof2exe::cna::analyze_file(path)?;
                let name = self.command.as_deref().unwrap_or_default();
                let commands: Vec<_> = report.commands.iter().filter(|c| c.name == name).collect();
                anyhow::ensure!(commands.len() == 1, "expected one CNA command {name:?}");
                anyhow::ensure!(
                    commands[0].entrypoints.len() <= 1,
                    "CNA has multiple entrypoints; select --entry explicitly"
                );
                input.entry = commands[0].entrypoints.first().cloned();
            }
        }
        Ok(input)
    }
}
