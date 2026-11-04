mod support;
use bof2exe::{
    coff::Coff,
    inspect::inspect,
    link::{combine, prepare, Input, ResourceInput, Spec},
};
use support::{object, Section, Symbol};

#[test]
fn resolves_cross_object_references_and_emits_deterministically() {
    let mut section = Section::text(vec![0xc3; 16]);
    section.relocations.push((8, 1, 1));
    let main = object(
        0x8664,
        false,
        &[section],
        &[
            Symbol::external("go", 1, 0),
            Symbol::external("helper", 0, 0),
        ],
    );
    let helper = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("helper", 1, 0)],
    );
    let inputs = [
        Input {
            name: "main",
            bytes: &main,
        },
        Input {
            name: "helper",
            bytes: &helper,
        },
    ];
    let linked = combine(&inputs, &[]).unwrap();
    assert_eq!(linked.bytes, combine(&inputs, &[]).unwrap().bytes);
    assert_eq!(&linked.bytes[4..8], &[0; 4]);
    assert!(linked.report.resolved.contains(&"helper".into()));
    assert!(linked.report.unresolved.is_empty());
    let file = Coff::parse(&linked.bytes).unwrap();
    let target = file.sections[0].relocations[0].symbol;
    assert_eq!(file.symbols[target].as_ref().unwrap().section, 2);
    assert!(inspect(&linked.bytes, "go").unwrap().compatible());
}
#[test]
fn common_symbols_allocate_largest_definition_and_normalize_single_inputs() {
    let mut section = Section::text(vec![0xc3; 16]);
    section.relocations.push((8, 1, 1));
    let main = object(
        0x8664,
        false,
        &[section],
        &[
            Symbol::external("go", 1, 0),
            Symbol::external("shared", 0, 16),
        ],
    );
    let other = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("shared", 0, 64)],
    );
    assert!(matches!(
        prepare(&main).unwrap(),
        std::borrow::Cow::Owned(_)
    ));
    assert!(inspect(&main, "go").unwrap().compatible());
    let linked = combine(
        &[
            Input {
                name: "main",
                bytes: &main,
            },
            Input {
                name: "other",
                bytes: &other,
            },
        ],
        &[],
    )
    .unwrap();
    let file = Coff::parse(&linked.bytes).unwrap();
    let shared = file
        .symbols
        .iter()
        .flatten()
        .find(|s| s.name == "shared" && s.section > 0)
        .unwrap();
    assert_eq!(file.sections[shared.section as usize - 1].size, 64);
    assert!(file.sections[shared.section as usize - 1].data.is_empty());
}
fn weak(name: &str, fallback: u32) -> Symbol<'_> {
    let mut aux = vec![0; 18];
    aux[..4].copy_from_slice(&fallback.to_le_bytes());
    aux[4..8].copy_from_slice(&3u32.to_le_bytes());
    Symbol {
        name,
        value: 0,
        section: 0,
        storage: 105,
        auxiliary: vec![aux],
    }
}
#[test]
fn weak_aliases_resolve_and_cycles_fail() {
    let mut section = Section::text(vec![0xc3; 16]);
    section.relocations.push((8, 1, 1));
    let bytes = object(
        0x8664,
        false,
        &[section],
        &[
            Symbol::external("go", 1, 0),
            weak("alias", 3),
            Symbol::external("fallback", 1, 0),
        ],
    );
    let linked = prepare(&bytes).unwrap();
    let file = Coff::parse(&linked).unwrap();
    let symbol = file.symbols[file.sections[0].relocations[0].symbol]
        .as_ref()
        .unwrap();
    assert_eq!(symbol.name, "fallback");
    let bytes = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("go", 1, 0), weak("a", 3), weak("b", 1)],
    );
    assert!(prepare(&bytes).unwrap_err().to_string().contains("cyclic"));
}
#[test]
fn rejects_duplicate_definitions_and_mixed_architectures() {
    let bytes = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("go", 1, 0)],
    );
    assert!(combine(
        &[
            Input {
                name: "one",
                bytes: &bytes
            },
            Input {
                name: "two",
                bytes: &bytes
            }
        ],
        &[]
    )
    .err()
    .unwrap()
    .to_string()
    .contains("duplicate"));
    let x86 = object(
        0x14c,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("other", 1, 0)],
    );
    assert!(combine(
        &[
            Input {
                name: "one",
                bytes: &bytes
            },
            Input {
                name: "two",
                bytes: &x86
            }
        ],
        &[]
    )
    .is_err());
}
#[test]
fn embeds_resources_and_length_symbols() {
    let bytes = object(
        0x8664,
        false,
        &[Section::text(vec![0xc3])],
        &[Symbol::external("go", 1, 0)],
    );
    let linked = combine(
        &[Input {
            name: "one",
            bytes: &bytes,
        }],
        &[ResourceInput {
            symbol: "greeting",
            bytes: b"abc",
        }],
    )
    .unwrap();
    let file = Coff::parse(&linked.bytes).unwrap();
    let resource = file
        .symbols
        .iter()
        .flatten()
        .find(|s| s.name == "greeting")
        .unwrap();
    let length = file
        .symbols
        .iter()
        .flatten()
        .find(|s| s.name == "greeting_len")
        .unwrap();
    let data = file.sections[resource.section as usize - 1].data;
    assert_eq!(&data[..3], b"abc");
    assert_eq!(&data[length.value as usize..], &3u32.to_le_bytes());
    assert!(combine(
        &[Input {
            name: "one",
            bytes: &bytes
        }],
        &[ResourceInput {
            symbol: "go",
            bytes: b"x"
        }]
    )
    .is_err());
}
#[test]
fn bigobj_and_extended_relocations_roundtrip() {
    let mut section = Section::text(vec![0xc3]);
    section.relocations = vec![(0, 0, 0); 65536];
    let bytes = object(0x8664, true, &[section], &[Symbol::external("go", 1, 0)]);
    let file = Coff::parse(&bytes).unwrap();
    assert!(file.bigobj);
    assert_eq!(file.sections[0].relocations.len(), 65536);
    assert_eq!(inspect(&bytes, "go").unwrap().format, "bigobj");
    let linked = combine(
        &[Input {
            name: "big",
            bytes: &bytes,
        }],
        &[],
    )
    .unwrap();
    assert_eq!(
        Coff::parse(&linked.bytes).unwrap().sections[0]
            .relocations
            .len(),
        65536
    );
    let mut invalid = bytes;
    invalid[12] ^= 1;
    assert!(Coff::parse(&invalid).is_err());
}
#[test]
fn link_spec_paths_resolve_beside_the_spec() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("build.toml");
    std::fs::write(&path,"schema=1\nname='fixture'\nobjects=['main.o','helper.o']\n[[resources]]\npath='data.txt'\nsymbol='config'\n").unwrap();
    let spec = Spec::load(&path).unwrap();
    assert_eq!(spec.objects[0], dir.path().join("main.o"));
    assert_eq!(spec.resources[0].path, dir.path().join("data.txt"));
    std::fs::write(&path, "schema=99\nname='bad'\nobjects=[]").unwrap();
    assert!(Spec::load(&path).is_err());
}
