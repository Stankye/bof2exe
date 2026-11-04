//! Standalone download artifacts. This is local output, not a C2 transport.
use crate::runtime::Output;
use anyhow::{bail, ensure, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Reserved output kind for a copied filename/buffer pair.
pub const KIND: i32 = i32::MIN;
const MAGIC: &[u8; 6] = b"B2EDL\x01";
pub(crate) const MAX_NAME: usize = 255;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A validated filename and the original binary bytes, borrowed from captured output.
pub struct Download<'a> {
    pub filename: &'a str,
    pub data: &'a [u8],
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= MAX_NAME,
        "invalid download filename length"
    );
    ensure!(
        !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
            && !name.ends_with(['.', ' ']),
        "download filename must be a single Windows-compatible filename"
    );
    let stem = name.split('.').next().unwrap().trim_end().to_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            ["1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"].contains(&suffix)
        })
    });
    ensure!(!device, "reserved Windows download filename");
    Ok(())
}

/// Copy an artifact into a bounded output event. Native callers may free their buffer afterwards.
pub fn capture(filename: &str, data: &[u8]) -> Result<Output> {
    validate_name(filename)?;
    let length = 8usize
        .checked_add(filename.len())
        .and_then(|v| v.checked_add(data.len()))
        .context("download length overflow")?;
    ensure!(
        length <= crate::MAX_INPUT,
        "download exceeds the 64 MiB capture limit"
    );
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length)?;
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(filename.len() as u16).to_le_bytes());
    bytes.extend_from_slice(filename.as_bytes());
    bytes.extend_from_slice(data);
    Ok(Output {
        kind: KIND,
        data: bytes,
    })
}

/// Decode an artifact event; ordinary callback events return None.
pub fn decode(output: &Output) -> Result<Option<Download<'_>>> {
    if output.kind != KIND {
        return Ok(None);
    }
    let bytes = &output.data;
    ensure!(
        bytes.len() >= 8 && bytes.len() <= crate::MAX_INPUT && bytes.starts_with(MAGIC),
        "invalid download event"
    );
    let length = u16::from_le_bytes(bytes[6..8].try_into()?) as usize;
    let name_end = 8usize
        .checked_add(length)
        .context("download filename overflow")?;
    ensure!(name_end <= bytes.len(), "truncated download filename");
    let filename = std::str::from_utf8(&bytes[8..name_end])?;
    validate_name(filename)?;
    Ok(Some(Download {
        filename,
        data: &bytes[name_end..],
    }))
}

fn create_run_directory(base: &Path) -> Result<PathBuf> {
    let time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    for _ in 0..16 {
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!(
            "bof2exe-downloads-{}-{time}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error).context("create download directory"),
        }
    }
    bail!("could not allocate a unique download directory")
}

/// Write ordinary callbacks to their streams and downloads into a fresh directory under base.
/// Each download has its own numbered subdirectory, preserving duplicate filenames without overwrite.
pub fn write_output(
    output: &[Output],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    base: &Path,
) -> Result<Vec<PathBuf>> {
    let mut directory = None;
    let mut files = Vec::new();
    for item in output {
        if let Some(download) = decode(item)? {
            let root = match &directory {
                Some(path) => path,
                None => directory.insert(create_run_directory(base)?),
            };
            let folder = root.join(format!("{:05}", files.len() + 1));
            fs::create_dir(&folder)?;
            let path = folder.join(download.filename);
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            if let Err(error) = file.write_all(download.data).and_then(|()| file.flush()) {
                drop(file);
                let _ = fs::remove_file(&path);
                return Err(error).context("write download artifact");
            }
            writeln!(
                stdout,
                "[download] {} ({} bytes)",
                path.display(),
                download.data.len()
            )?;
            files.push(path);
        } else if item.kind == 13 {
            stderr.write_all(&item.data)?;
        } else {
            stdout.write_all(&item.data)?;
        }
    }
    Ok(files)
}
