use bof2exe::{coff::Coff, inspect::inspect};

fn word(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn dword(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn minimal() -> Vec<u8> {
    let mut bytes = vec![0; 83];
    word(&mut bytes, 0, 0x8664);
    word(&mut bytes, 2, 1);
    dword(&mut bytes, 8, 61);
    dword(&mut bytes, 12, 1);
    bytes[20..25].copy_from_slice(b".text");
    dword(&mut bytes, 36, 1);
    dword(&mut bytes, 40, 60);
    dword(&mut bytes, 56, 0x60500020);
    bytes[60] = 0xc3;
    bytes[61..63].copy_from_slice(b"go");
    word(&mut bytes, 73, 1);
    bytes[77] = 2;
    dword(&mut bytes, 79, 4);
    bytes
}
#[test]
fn minimal_standard_coff_and_every_truncation() {
    let bytes = minimal();
    assert!(inspect(&bytes, "go").unwrap().compatible());
    for len in 0..bytes.len() {
        assert!(
            Coff::parse(&bytes[..len]).is_err(),
            "accepted truncation at {len}"
        );
    }
    assert!(!inspect(&bytes, "missing").unwrap().compatible());
}
#[test]
fn malformed_tables_are_errors_instead_of_panics() {
    let original = minimal();
    for (at, value) in [
        (8, u32::MAX),
        (12, u32::MAX),
        (36, u32::MAX),
        (40, u32::MAX),
        (79, u32::MAX),
    ] {
        let mut bytes = original.clone();
        dword(&mut bytes, at, value);
        assert!(Coff::parse(&bytes).is_err(), "accepted mutation at {at}");
    }
    let mut bytes = original.clone();
    bytes[78] = 255;
    assert!(Coff::parse(&bytes).is_err());
    let mut bytes = original.clone();
    word(&mut bytes, 73, 2);
    assert!(Coff::parse(&bytes).is_err());
    let mut bytes = original;
    bytes[61..65].fill(0);
    dword(&mut bytes, 65, 99999);
    assert!(Coff::parse(&bytes).is_err());
}
#[test]
fn architecture_and_writable_code_fail_compatibility() {
    let mut bytes = minimal();
    word(&mut bytes, 0, 0x14c);
    assert!(!inspect(&bytes, "go").unwrap().compatible());
    word(&mut bytes, 0, 0x8664);
    dword(&mut bytes, 56, 0xe0500020);
    assert!(inspect(&bytes, "go")
        .unwrap()
        .issues
        .iter()
        .any(|v| v.contains("writable executable")));
}
#[test]
fn random_input_never_panics() {
    let mut state = 1u64;
    for len in 0..512 {
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let _ = Coff::parse(&bytes);
    }
}
