mod support;
use bof2exe::{library, link::Input};
use support::{archive, object, zip, Section, Symbol};

fn unit(name: &str, reference: Option<&str>) -> Vec<u8> {
    let mut section = Section::text(vec![0xc3; 16]);
    let mut symbols = vec![Symbol::external(name, 1, 0)];
    if let Some(reference) = reference {
        section.relocations.push((8, 1, 1));
        symbols.push(Symbol::external(reference, 0, 0));
    }
    object(0x8664, false, &[section], &symbols)
}
#[test]
fn archive_longnames_and_bsd_inline_names_are_read_without_extraction() {
    let bytes = unit("helper", None);
    for names in [
        b"long-object-name.o\0".as_slice(),
        b"long-object-name.o/\n".as_slice(),
    ] {
        let ar = archive(&[("/", b"index"), ("//", names), ("/0", &bytes)]);
        let read = library::read(&ar).unwrap();
        assert_eq!(read.members[0].name, "long-object-name.o");
        assert_eq!(&*read.members[0].bytes, bytes);
    }
    let mut inline = b"inline-name.o".to_vec();
    inline.extend(&bytes);
    let ar = archive(&[("#1/13", &inline)]);
    assert_eq!(library::read(&ar).unwrap().members[0].name, "inline-name.o");
}
#[test]
fn stored_and_deflated_zip_collections_verify_objects_and_ignore_metadata() {
    let bytes = unit("helper", None);
    for deflate in [false, true] {
        let zip = zip(
            &[("x64/helper.o", &bytes), ("README.md", b"metadata")],
            deflate,
        );
        let library = library::read(&zip).unwrap();
        assert_eq!(library.format, "zip-coff");
        assert_eq!(library.members.len(), 1);
        assert_eq!(&*library.members[0].bytes, bytes);
        assert_eq!(library.ignored, ["README.md"]);
        assert_eq!(library.report().unwrap().members[0].definitions, ["helper"]);
    }
}
#[test]
fn lazy_selection_resolves_transitive_members_across_libraries() {
    let root = unit("go", Some("helper"));
    let helper = unit("helper", Some("utility"));
    let utility = unit("utility", None);
    let conflict = unit("go", None);
    let first = archive(&[("utility.o/", &utility)]);
    let second = zip(&[("helper.o", &helper), ("conflict.o", &conflict)], true);
    let roots = [Input {
        name: "root",
        bytes: &root,
    }];
    let libraries = [
        Input {
            name: "utility.lib",
            bytes: &first,
        },
        Input {
            name: "helpers.zip",
            bytes: &second,
        },
    ];
    let (linked, selection) = library::link(&roots, &libraries, &[], false).unwrap();
    assert_eq!(
        selection.selected,
        ["utility.lib(utility.o)", "helpers.zip(helper.o)"]
    );
    assert_eq!(selection.unused, ["helpers.zip(conflict.o)"]);
    assert!(linked.report.unresolved.is_empty());
    assert!(library::link(&roots, &libraries, &[], true).is_err());
}
#[test]
fn unused_members_from_other_architectures_are_not_selected() {
    let root = unit("go", Some("helper"));
    let helper = unit("helper", None);
    let mut x86 = helper.clone();
    x86[..2].copy_from_slice(&0x14cu16.to_le_bytes());
    let ar = archive(&[("x86.o/", &x86), ("x64.o/", &helper)]);
    let (_, selection) = library::link(
        &[Input {
            name: "root",
            bytes: &root,
        }],
        &[Input {
            name: "mixed.lib",
            bytes: &ar,
        }],
        &[],
        false,
    )
    .unwrap();
    assert_eq!(selection.selected, ["mixed.lib(x64.o)"]);
}
#[test]
fn invalid_archives_and_zip_metadata_fail_before_allocation_or_execution() {
    assert!(library::read(b"!<thin>\n").is_err());
    let bytes = unit("helper", None);
    let ar = archive(&[("helper.o/", &bytes)]);
    for length in 0..ar.len() {
        if length == 8 {
            continue;
        }
        assert!(
            library::read(&ar[..length]).is_err(),
            "accepted archive truncation {length}"
        );
    }
    let mut duplicate = zip(&[("same.o", &bytes), ("same.o", &bytes)], false);
    assert!(library::read(&duplicate).is_err());
    duplicate = zip(&[("helper.o", &bytes)], false);
    let payload = 30 + "helper.o".len();
    duplicate[payload] ^= 1;
    assert!(library::read(&duplicate).unwrap_err_text().contains("CRC"));
}
trait ErrorText {
    fn unwrap_err_text(self) -> String;
}
impl<T> ErrorText for anyhow::Result<T> {
    fn unwrap_err_text(self) -> String {
        match self {
            Ok(_) => panic!("expected error"),
            Err(e) => e.to_string(),
        }
    }
}
#[test]
fn zip_length_limits_encryption_and_every_truncation_are_rejected() {
    let bytes = unit("helper", None);
    let original = zip(&[("helper.o", &bytes)], true);
    for length in 0..original.len() {
        assert!(
            library::read(&original[..length]).is_err(),
            "accepted ZIP truncation {length}"
        );
    }
    let central = original
        .windows(4)
        .position(|v| v == b"PK\x01\x02")
        .unwrap();
    let mut hostile = original.clone();
    hostile[central + 24..central + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(library::read(&hostile).is_err());
    let mut encrypted = original;
    encrypted[central + 8] |= 1;
    assert!(library::read(&encrypted).is_err());
}
#[test]
fn library_cli_reports_metadata_as_json() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("helper.lib");
    let bytes = unit("helper", None);
    std::fs::write(&path, archive(&[("helper.o/", &bytes)])).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_bof2exe"))
        .arg("library")
        .arg(path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["members"][0]["definitions"][0], "helper");
}

#[test]
fn repeated_ar_member_names_remain_distinct_and_have_stable_report_labels() {
    let mut root = Section::text(vec![0xc3; 24]);
    root.relocations = vec![(8, 1, 1), (16, 2, 1)];
    let root = object(
        0x8664,
        false,
        &[root],
        &[
            Symbol::external("go", 1, 0),
            Symbol::external("first", 0, 0),
            Symbol::external("second", 0, 0),
        ],
    );
    let first = unit("first", None);
    let second = unit("second", None);
    let ar = archive(&[("same.o/", &first), ("same.o/", &second)]);
    let (_, report) = library::link(
        &[Input {
            name: "root",
            bytes: &root,
        }],
        &[Input {
            name: "library",
            bytes: &ar,
        }],
        &[],
        false,
    )
    .unwrap();
    assert_eq!(report.selected, ["library(1:same.o)", "library(2:same.o)"]);
}
#[test]
fn weak_library_search_and_nolibrary_fallback_have_different_selection_rules() {
    let alias = unit("alias", None);
    let ar = archive(&[("alias.o/", &alias)]);
    for (mode, count) in [(1u32, 0usize), (2u32, 1usize)] {
        let mut section = Section::text(vec![0xc3; 16]);
        section.relocations.push((8, 1, 1));
        let mut aux = vec![0; 18];
        aux[..4].copy_from_slice(&3u32.to_le_bytes());
        aux[4..8].copy_from_slice(&mode.to_le_bytes());
        let root = object(
            0x8664,
            false,
            &[section],
            &[
                Symbol::external("go", 1, 0),
                Symbol {
                    name: "alias",
                    value: 0,
                    section: 0,
                    storage: 105,
                    auxiliary: vec![aux],
                },
                Symbol::external("fallback", 1, 0),
            ],
        );
        let (linked, report) = library::link(
            &[Input {
                name: "root",
                bytes: &root,
            }],
            &[Input {
                name: "library",
                bytes: &ar,
            }],
            &[],
            false,
        )
        .unwrap();
        assert_eq!(report.selected.len(), count);
        let file = bof2exe::coff::Coff::parse(&linked.bytes).unwrap();
        let target = file.symbols[file.sections[0].relocations[0].symbol]
            .as_ref()
            .unwrap();
        assert_eq!(target.name, if mode == 1 { "fallback" } else { "alias" });
    }
}
#[test]
fn discarded_comdat_references_do_not_pull_extra_archive_members() {
    let mut first = Section::text(vec![0xc3; 16]);
    first.flags |= 0x1000;
    let first = object(
        0x8664,
        false,
        &[first],
        &[
            support::section_symbol(".text", 1, 16, 2, 0),
            Symbol::external("go", 1, 0),
        ],
    );
    let mut second = Section::text(vec![0xc3; 16]);
    second.flags |= 0x1000;
    second.relocations.push((8, 3, 1));
    let second = object(
        0x8664,
        false,
        &[second],
        &[
            support::section_symbol(".text", 1, 16, 2, 0),
            Symbol::external("go", 1, 0),
            Symbol::external("helper", 0, 0),
        ],
    );
    let helper = unit("helper", None);
    let ar = archive(&[("helper.o/", &helper)]);
    let (linked, report) = library::link(
        &[
            Input {
                name: "first",
                bytes: &first,
            },
            Input {
                name: "second",
                bytes: &second,
            },
        ],
        &[Input {
            name: "library",
            bytes: &ar,
        }],
        &[],
        false,
    )
    .unwrap();
    assert!(report.selected.is_empty());
    assert!(linked.report.unresolved.is_empty());
}
