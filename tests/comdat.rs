mod support;
use bof2exe::{
    coff::Coff,
    link::{combine, Input},
};
use support::{object, section_symbol, Section, Symbol};

fn grouped(mode: u8, data: Vec<u8>, associated: bool) -> Vec<u8> {
    let size = data.len() as u32;
    let mut root = Section::text(data);
    root.name = ".text$go";
    root.flags |= 0x1000;
    let mut sections = vec![root];
    let mut symbols = vec![
        section_symbol(".text$go", 1, size, mode, 0),
        Symbol::external("go", 1, 0),
    ];
    if associated {
        sections.push(Section {
            name: ".xdata",
            data: vec![mode, 0, 0, 0],
            size: 4,
            flags: 0x40301040,
            relocations: Vec::new(),
        });
        symbols.push(section_symbol(".xdata", 2, 4, 5, 1));
    }
    object(0x8664, false, &sections, &symbols)
}
#[test]
fn any_and_associative_sections_are_selected_as_a_group() {
    let first = grouped(2, vec![0xc3], true);
    let second = grouped(2, vec![0x90, 0xc3], true);
    let linked = combine(
        &[
            Input {
                name: "a",
                bytes: &first,
            },
            Input {
                name: "b",
                bytes: &second,
            },
        ],
        &[],
    )
    .unwrap();
    let file = Coff::parse(&linked.bytes).unwrap();
    assert_eq!(file.sections.len(), 2);
    assert_eq!(file.sections[0].data, [0xc3]);
    assert_eq!(file.sections[1].data, [2, 0, 0, 0]);
    assert_eq!(linked.report.comdat_discarded.len(), 2);
    assert!(file.sections.iter().all(|s| s.flags & 0x1000 == 0));
}
#[test]
fn duplicate_noduplicates_and_mismatched_same_size_or_exact_match_fail() {
    for mode in [1, 3, 4] {
        let first = grouped(mode, vec![0xc3], false);
        let second = grouped(mode, vec![0x90, 0xc3], false);
        assert!(
            combine(
                &[
                    Input {
                        name: "a",
                        bytes: &first
                    },
                    Input {
                        name: "b",
                        bytes: &second
                    }
                ],
                &[]
            )
            .is_err(),
            "mode {mode}"
        );
    }
    for mode in [3, 4] {
        let first = grouped(mode, vec![0xc3], false);
        let linked = combine(
            &[
                Input {
                    name: "a",
                    bytes: &first,
                },
                Input {
                    name: "b",
                    bytes: &first,
                },
            ],
            &[],
        )
        .unwrap();
        assert_eq!(Coff::parse(&linked.bytes).unwrap().sections.len(), 1);
    }
}
#[test]
fn largest_and_newest_choose_the_expected_definition() {
    let small = grouped(6, vec![0xc3], false);
    let large = grouped(6, vec![0x90, 0xc3], false);
    let linked = combine(
        &[
            Input {
                name: "a",
                bytes: &small,
            },
            Input {
                name: "b",
                bytes: &large,
            },
        ],
        &[],
    )
    .unwrap();
    assert_eq!(
        Coff::parse(&linked.bytes).unwrap().sections[0].data,
        [0x90, 0xc3]
    );
    let mut old = grouped(7, vec![0xc3], false);
    old[4..8].copy_from_slice(&1u32.to_le_bytes());
    let mut new = grouped(7, vec![0x90, 0xc3], false);
    new[4..8].copy_from_slice(&2u32.to_le_bytes());
    let linked = combine(
        &[
            Input {
                name: "old",
                bytes: &old,
            },
            Input {
                name: "new",
                bytes: &new,
            },
        ],
        &[],
    )
    .unwrap();
    assert_eq!(
        Coff::parse(&linked.bytes).unwrap().sections[0].data,
        [0x90, 0xc3]
    );
}
#[test]
fn a_live_external_reference_redirects_to_the_selected_definition() {
    let first = grouped(2, vec![0xc3], false);
    let second = grouped(2, vec![0x90, 0xc3], false);
    let mut caller = Section::text(vec![0xc3; 16]);
    caller.relocations.push((8, 1, 1));
    let caller = object(
        0x8664,
        false,
        &[caller],
        &[
            Symbol::external("caller", 1, 0),
            Symbol::external("go", 0, 0),
        ],
    );
    let linked = combine(
        &[
            Input {
                name: "a",
                bytes: &first,
            },
            Input {
                name: "b",
                bytes: &second,
            },
            Input {
                name: "caller",
                bytes: &caller,
            },
        ],
        &[],
    )
    .unwrap();
    let file = Coff::parse(&linked.bytes).unwrap();
    let target = file.sections[1].relocations[0].symbol;
    assert_eq!(file.symbols[target].as_ref().unwrap().section, 1);
}
#[test]
fn invalid_association_and_missing_auxiliary_data_are_errors() {
    let mut section = Section::text(vec![0xc3]);
    section.flags |= 0x1000;
    let missing = object(0x8664, false, &[section], &[Symbol::external("go", 1, 0)]);
    assert!(combine(
        &[Input {
            name: "bad",
            bytes: &missing
        }],
        &[]
    )
    .is_err());
    let mut section = Section::text(vec![0xc3]);
    section.flags |= 0x1000;
    let cycle = object(
        0x8664,
        false,
        &[section],
        &[
            section_symbol(".text", 1, 1, 5, 1),
            Symbol::external("go", 1, 0),
        ],
    );
    assert!(combine(
        &[Input {
            name: "bad",
            bytes: &cycle
        }],
        &[]
    )
    .is_err());
}

#[test]
fn exact_match_compares_symbol_meaning_instead_of_raw_table_indices() {
    fn reference(name: &str, extra: bool) -> Vec<u8> {
        let mut section = Section::text(vec![0xc3; 16]);
        section.name = ".text$go";
        section.flags |= 0x1000;
        let mut symbols = Vec::new();
        if extra {
            symbols.push(Symbol {
                name: "absolute",
                value: 1,
                section: -1,
                storage: 3,
                auxiliary: Vec::new(),
            });
        }
        symbols.push(section_symbol(".text$go", 1, 16, 4, 0));
        symbols.push(Symbol::external("go", 1, 0));
        let index = symbols.iter().map(|s| 1 + s.auxiliary.len()).sum::<usize>() as u32;
        symbols.push(Symbol::external(name, 0, 0));
        section.relocations.push((8, index, 1));
        object(0x8664, false, &[section], &symbols)
    }
    let first = reference("import", false);
    let same = reference("import", true);
    let different = reference("other", true);
    assert!(combine(
        &[
            Input {
                name: "first",
                bytes: &first
            },
            Input {
                name: "same",
                bytes: &same
            }
        ],
        &[]
    )
    .is_ok());
    assert!(combine(
        &[
            Input {
                name: "first",
                bytes: &first
            },
            Input {
                name: "different",
                bytes: &different
            }
        ],
        &[]
    )
    .is_err());
}
