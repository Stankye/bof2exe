mod support;
use support::{object, Section, Symbol};

#[test]
fn conventional_loader_imports_accept_direct_and_indirect_symbols() {
    for api in bof2exe::inspect::LOADER_APIS {
        for name in [api.to_string(), format!("__imp_{api}")] {
            let mut section = Section::text(vec![0; 8]);
            section.relocations.push((0, 1, 1));
            let bytes = object(
                0x8664,
                false,
                &[section],
                &[Symbol::external("go", 1, 0), Symbol::external(&name, 0, 0)],
            );
            assert!(
                bof2exe::inspect::inspect(&bytes, "go")
                    .unwrap()
                    .compatible(),
                "{name}"
            );
        }
    }
    let mut section = Section::text(vec![0; 8]);
    section.relocations.push((0, 1, 1));
    let bytes = object(
        0x8664,
        false,
        &[section],
        &[
            Symbol::external("go", 1, 0),
            Symbol::external("__imp_UnknownWindowsFunction", 0, 0),
        ],
    );
    assert!(!bof2exe::inspect::inspect(&bytes, "go")
        .unwrap()
        .compatible());
}
