//! Typed arguments using the Beacon length-prefixed, little-endian wire format.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArgumentError {
    #[error("expected TYPE:VALUE (str, wstr, int, short, bin, or hex)")]
    Syntax,
    #[error("invalid {kind} argument: {reason}")]
    Value { kind: String, reason: String },
    #[error("packed arguments exceed 64 MiB")]
    TooLarge,
}

/// Convert raw values using a literal Aggressor `bof_pack` signature.
/// `b` values are base64, `bin:BASE64`, or `hex:HEX`; no expressions are executed.
pub fn from_format(format: &str, values: &[String]) -> anyhow::Result<Vec<Argument>> {
    let types = format_types(format)?;
    anyhow::ensure!(
        types.len() == values.len(),
        "signature {format:?} expects {} values; received {}",
        types.len(),
        values.len()
    );
    types
        .iter()
        .zip(values)
        .enumerate()
        .map(|(i, (kind, value))| {
            let text = if *kind == "bin" && (value.starts_with("bin:") || value.starts_with("hex:"))
            {
                value.clone()
            } else {
                format!("{kind}:{value}")
            };
            text.parse()
                .map_err(|error| anyhow::anyhow!("parameter {} ({kind}): {error}", i + 1))
        })
        .collect()
}

/// Ordered wire types for the documented `b`, `i`, `s`, `z`, and `Z` formats.
pub fn format_types(format: &str) -> anyhow::Result<Vec<&'static str>> {
    anyhow::ensure!(format.len() <= 4096, "format exceeds 4096 parameters");
    format
        .chars()
        .map(|c| match c {
            'b' => Ok("bin"),
            'i' => Ok("int"),
            's' => Ok("short"),
            'z' => Ok("str"),
            'Z' => Ok("wstr"),
            _ => anyhow::bail!("unsupported bof_pack format character {c:?}"),
        })
        .collect()
}

/// A typed argument, also accepted in module manifests.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "value", rename_all = "lowercase")]
pub enum Argument {
    Str(String),
    Wstr(String),
    Int(i32),
    Short(i16),
    Bin(Vec<u8>),
}

impl std::str::FromStr for Argument {
    type Err = ArgumentError;
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let (kind, value) = text.split_once(':').ok_or(ArgumentError::Syntax)?;
        let bad = |reason: String| ArgumentError::Value {
            kind: kind.into(),
            reason,
        };
        Ok(match kind {
            "str" | "wstr" => {
                if value.contains('\0') {
                    return Err(bad("interior NUL is not allowed".into()));
                }
                if kind == "str" {
                    Self::Str(value.into())
                } else {
                    Self::Wstr(value.into())
                }
            }
            "int" => Self::Int(value.parse::<i32>().map_err(|e| bad(e.to_string()))?),
            "short" => Self::Short(value.parse::<i16>().map_err(|e| bad(e.to_string()))?),
            "bin" => Self::Bin(STANDARD.decode(value).map_err(|e| bad(e.to_string()))?),
            "hex" => {
                if value.len() % 2 != 0 || !value.is_ascii() {
                    return Err(bad("expected an even number of ASCII hex digits".into()));
                }
                let data = value
                    .as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| {
                        let digits = std::str::from_utf8(pair).map_err(|e| bad(e.to_string()))?;
                        u8::from_str_radix(digits, 16).map_err(|e| bad(e.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Self::Bin(data)
            }
            _ => return Err(ArgumentError::Syntax),
        })
    }
}

/// Pack arguments including the four-byte length header expected by BeaconDataParse.
pub fn pack(arguments: &[Argument]) -> Result<Vec<u8>, ArgumentError> {
    let mut out = vec![0u8; 4];
    for arg in arguments {
        match arg {
            Argument::Int(value) => out.extend(value.to_le_bytes()),
            Argument::Short(value) => out.extend(value.to_le_bytes()),
            Argument::Str(value) => {
                if value.contains('\0') {
                    return Err(ArgumentError::Value {
                        kind: "str".into(),
                        reason: "interior NUL".into(),
                    });
                }
                blob(&mut out, value.as_bytes(), &[0])?;
            }
            Argument::Wstr(value) => {
                if value.contains('\0') {
                    return Err(ArgumentError::Value {
                        kind: "wstr".into(),
                        reason: "interior NUL".into(),
                    });
                }
                let data: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
                blob(&mut out, &data, &[0, 0])?;
            }
            Argument::Bin(value) => blob(&mut out, value, &[])?,
        }
        if out.len() > crate::MAX_INPUT {
            return Err(ArgumentError::TooLarge);
        }
    }
    let len = (out.len() - 4) as u32;
    out[..4].copy_from_slice(&len.to_le_bytes());
    Ok(out)
}

fn blob(out: &mut Vec<u8>, data: &[u8], suffix: &[u8]) -> Result<(), ArgumentError> {
    let len = data
        .len()
        .checked_add(suffix.len())
        .ok_or(ArgumentError::TooLarge)?;
    if len > crate::MAX_INPUT || out.len().saturating_add(4).saturating_add(len) > crate::MAX_INPUT
    {
        return Err(ArgumentError::TooLarge);
    }
    out.extend((len as u32).to_le_bytes());
    out.extend(data);
    out.extend(suffix);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_format_matches_beacon() {
        let args = ["int:16909060", "short:-2", "str:a", "wstr:😀", "hex:00ff"]
            .map(|s| s.parse().unwrap());
        let data = pack(&args).unwrap();
        assert_eq!(
            data,
            [
                28, 0, 0, 0, 4, 3, 2, 1, 254, 255, 2, 0, 0, 0, 97, 0, 6, 0, 0, 0, 61, 216, 0, 222,
                0, 0, 2, 0, 0, 0, 0, 255
            ]
        );
    }
    #[test]
    fn rejects_bad_values() {
        for value in [
            "int:2147483648",
            "short:32768",
            "hex:0",
            "hex:xx",
            "hex:é",
            "bin:?",
            "str:a\0b",
            "nope:1",
            "oops",
        ] {
            assert!(value.parse::<Argument>().is_err(), "{value}");
        }
        assert_eq!(
            "str:C:\\foo:bar".parse::<Argument>().unwrap(),
            Argument::Str("C:\\foo:bar".into())
        );
        assert_eq!(pack(&[]).unwrap(), [0, 0, 0, 0]);
    }
}
