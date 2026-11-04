//! Versioned executable overlays with corruption detection. SHA-256 is not a signature.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};

const MAGIC: &[u8; 16] = b"BOF2EXE_BUNDLE01";
const FOOTER: usize = 56;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub schema: u32,
    pub entry: String,
    pub object_base64: String,
    pub arguments_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<crate::tool::Definition>,
}
impl Payload {
    pub fn new(object: &[u8], entry: String, arguments: &[u8]) -> Self {
        Self {
            schema: 1,
            entry,
            object_base64: STANDARD.encode(object),
            arguments_base64: STANDARD.encode(arguments),
            tool: None,
        }
    }
    pub fn decode(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        ensure!(
            self.schema == 1,
            "unsupported payload schema {}",
            self.schema
        );
        let encoded_limit = crate::MAX_INPUT.div_ceil(3) * 4;
        ensure!(
            self.object_base64.len() <= encoded_limit
                && self.arguments_base64.len() <= encoded_limit,
            "encoded input exceeds 64 MiB decoded limit"
        );
        let object = STANDARD.decode(&self.object_base64)?;
        let arguments = STANDARD.decode(&self.arguments_base64)?;
        ensure!(
            object.len() <= crate::MAX_INPUT && arguments.len() <= crate::MAX_INPUT,
            "payload exceeds 64 MiB"
        );
        Ok((object, arguments))
    }
}

/// Append an overlay to a runner. The caller owns output creation/overwrite policy.
pub fn write(mut runner: impl Read, mut output: impl Write, payload: &Payload) -> Result<()> {
    let data = serde_json::to_vec(payload)?;
    ensure!(
        data.len() <= crate::MAX_INPUT,
        "encoded bundle payload exceeds 64 MiB"
    );
    std::io::copy(&mut runner, &mut output)?;
    output.write_all(&data)?;
    output.write_all(&(data.len() as u64).to_le_bytes())?;
    output.write_all(&Sha256::digest(&data))?;
    output.write_all(MAGIC)?;
    Ok(())
}

/// Read an embedded payload, or `None` when no overlay marker exists.
pub fn read(reader: &mut (impl Read + Seek)) -> Result<Option<Payload>> {
    let size = reader.seek(SeekFrom::End(0))?;
    if size < FOOTER as u64 {
        return Ok(None);
    }
    reader.seek(SeekFrom::End(-(FOOTER as i64)))?;
    let mut footer = [0; FOOTER];
    reader.read_exact(&mut footer)?;
    if &footer[40..] != MAGIC {
        return Ok(None);
    }
    let length = u64::from_le_bytes(footer[..8].try_into()?);
    ensure!(
        length <= crate::MAX_INPUT as u64 && length <= size - FOOTER as u64,
        "invalid bundle length"
    );
    reader.seek(SeekFrom::Start(size - FOOTER as u64 - length))?;
    let mut data = vec![0; length as usize];
    reader.read_exact(&mut data)?;
    ensure!(
        Sha256::digest(&data)[..] == footer[8..40],
        "bundle checksum mismatch"
    );
    let payload: Payload = serde_json::from_slice(&data).context("invalid bundle payload")?;
    ensure!(payload.schema == 1, "unsupported bundle schema");
    Ok(Some(payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn roundtrip_and_corruption() {
        let payload = Payload::new(b"object", "go".into(), &[0, 0, 0, 0]);
        let mut output = Vec::new();
        write(&b"runner"[..], &mut output, &payload).unwrap();
        let restored = read(&mut Cursor::new(&output)).unwrap().unwrap();
        assert_eq!(
            restored.decode().unwrap(),
            (b"object".to_vec(), vec![0, 0, 0, 0])
        );
        output[7] ^= 1;
        assert!(read(&mut Cursor::new(&output)).is_err());
        assert!(read(&mut Cursor::new(b"normal executable"))
            .unwrap()
            .is_none());
    }
    #[test]
    fn hostile_length_is_rejected_before_allocation() {
        let mut bytes = vec![0xff; 40];
        bytes.extend(MAGIC);
        assert!(read(&mut Cursor::new(bytes)).is_err());
    }
}
