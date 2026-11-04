use bof2exe::{downloads, runtime::Output, MAX_INPUT};

#[test]
fn downloads_own_the_binary_buffer_and_preserve_utf8_names() {
    let mut data = vec![0, 255, 10, 0, 128];
    let event = downloads::capture("résultat.json", &data).unwrap();
    data.fill(42);
    let serialized = serde_json::to_vec(&event).unwrap();
    let restored: Output = serde_json::from_slice(&serialized).unwrap();
    let download = downloads::decode(&restored).unwrap().unwrap();
    assert_eq!(download.filename, "résultat.json");
    assert_eq!(download.data, &[0, 255, 10, 0, 128]);
    let empty = downloads::capture("empty.bin", &[]).unwrap();
    assert!(downloads::decode(&empty).unwrap().unwrap().data.is_empty());
}

#[test]
fn rejects_paths_devices_and_ambiguous_windows_names() {
    for name in [
        "",
        ".",
        "..",
        "../out",
        "..\\out",
        "/tmp/out",
        "C:\\out",
        "\\\\host\\share",
        "a:b",
        "a\0b",
        "a\nb",
        "tail.",
        "tail ",
        "CON",
        "con.txt",
        "NUL.json",
        "AUX",
        "PRN.txt",
        "COM1.log",
        "LPT9",
        "COM¹",
        "CONIN$",
        "CONOUT$",
    ] {
        assert!(downloads::capture(name, b"data").is_err(), "{name:?}");
    }
    assert!(downloads::capture(&"x".repeat(256), b"").is_err());
    assert!(downloads::capture("COM10.json", b"").is_ok());
    assert!(downloads::capture("AZUser_20261104.json", b"{}").is_ok());
}

#[test]
fn rejects_truncated_forged_and_oversized_download_events() {
    let valid = downloads::capture("good.json", b"{}").unwrap();
    for end in 0..17 {
        let mut truncated = valid.clone();
        truncated.data.truncate(end);
        assert!(downloads::decode(&truncated).is_err(), "length {end}");
    }
    let mut invalid = valid.clone();
    invalid.data[0] = 0;
    assert!(downloads::decode(&invalid).is_err());
    let mut invalid = valid.clone();
    invalid.data[8] = 255;
    assert!(downloads::decode(&invalid).is_err());
    let mut invalid = valid.clone();
    invalid.data[6..8].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(downloads::decode(&invalid).is_err());
    let mut ordinary = valid;
    ordinary.kind = 0;
    assert!(downloads::decode(&ordinary).unwrap().is_none());
}

#[test]
fn capture_limit_includes_filename_and_framing() {
    let bytes = vec![0; MAX_INPUT - 9];
    assert_eq!(
        downloads::capture("x", &bytes).unwrap().data.len(),
        MAX_INPUT
    );
    assert!(downloads::capture("xx", &bytes).is_err());
}

#[test]
fn artifact_files_preserve_duplicate_names_and_callback_streams() {
    let base = tempfile::tempdir().unwrap();
    let events = vec![
        Output {
            kind: 0,
            data: b"before\n".to_vec(),
        },
        downloads::capture("same.bin", &[0, 255, 1]).unwrap(),
        Output {
            kind: 13,
            data: b"diagnostic\n".to_vec(),
        },
        downloads::capture("same.bin", b"second").unwrap(),
    ];
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let paths = downloads::write_output(&events, &mut stdout, &mut stderr, base.path()).unwrap();
    assert_eq!(paths.len(), 2);
    assert_ne!(paths[0], paths[1]);
    assert!(paths
        .iter()
        .all(|p| p.starts_with(base.path()) && p.file_name().unwrap() == "same.bin"));
    assert_eq!(std::fs::read(&paths[0]).unwrap(), [0, 255, 1]);
    assert_eq!(std::fs::read(&paths[1]).unwrap(), b"second");
    assert!(stdout.starts_with(b"before\n"));
    assert_eq!(stderr, b"diagnostic\n");
    let another =
        downloads::write_output(&events[1..2], &mut Vec::new(), &mut Vec::new(), base.path())
            .unwrap();
    assert_ne!(paths[0], another[0]);
    assert_eq!(std::fs::read(&paths[0]).unwrap(), [0, 255, 1]);
}

#[test]
fn write_failures_are_errors_and_callbacks_do_not_create_artifact_directories() {
    let base = tempfile::tempdir().unwrap();
    let ordinary = [Output {
        kind: 0,
        data: b"plain".to_vec(),
    }];
    let mut stdout = Vec::new();
    assert!(
        downloads::write_output(&ordinary, &mut stdout, &mut Vec::new(), base.path())
            .unwrap()
            .is_empty()
    );
    assert_eq!(std::fs::read_dir(base.path()).unwrap().count(), 0);
    assert_eq!(stdout, b"plain");
    let blocked = base.path().join("not-a-directory");
    std::fs::write(&blocked, b"original").unwrap();
    let event = downloads::capture("data.bin", b"new").unwrap();
    assert!(downloads::write_output(&[event], &mut Vec::new(), &mut Vec::new(), &blocked).is_err());
    assert_eq!(std::fs::read(&blocked).unwrap(), b"original");
}
