use super::*;
use crate::{compile_sources_with_options, CompileOptions};
use std::{collections::BTreeMap, path::PathBuf};

fn fixture() -> (CompileResult, CompileResult, LineMap) {
    let root = std::env::temp_dir().join("wl-entity-proof-only");
    let entry = root.join("world.wl");
    let source = root.join("source.wl");
    let destination = root.join("destination.wl");
    let block =
        "entity tower kind place as \"灯塔\"\n  description \"原说明\"\n  property height = 38\n";
    let retained = concat!(
        "tag lit as \"亮\"\nstate lamp on entity tower with lit\n",
        "event start\n  choice \"第 9 行🙂\"\n    if true\n      become lamp with lit\n    -> END\n",
    );
    let original_target = "entity keeper kind organization\n";
    let sources = BTreeMap::from([
        (
            entry.clone(),
            "include \"source.wl\"\ninclude \"destination.wl\"\n".into(),
        ),
        (source.clone(), format!("{block}{retained}")),
        (destination.clone(), original_target.into()),
    ]);
    let mut moved = sources.clone();
    moved.insert(source.clone(), retained.into());
    moved.insert(destination.clone(), format!("{original_target}{block}"));
    let before = compile_sources_with_options(&entry, &sources, CompileOptions::v1_10());
    let after = compile_sources_with_options(&entry, &moved, CompileOptions::v1_10());
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    assert!(!after.has_errors(), "{:?}", after.diagnostics);
    let map = LineMap {
        source: source.to_string_lossy().into(),
        destination: destination.to_string_lossy().into(),
        first: 1,
        last: 3,
        inserted: 2,
        removed_newlines: 3,
    };
    (before, after, map)
}

#[test]
fn actual_line_offsets_preserve_condition_context_and_unmoved_file_identity() {
    let (before, after, map) = fixture();
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    equivalent(&before, &after, &map).unwrap();
    let file = crate::catalog::TargetRef::new("file", &map.source);
    assert_eq!(
        after.analysis.catalog.object(&file).unwrap().file,
        map.source
    );
    let state = &after.analysis.catalog.states["lamp"];
    assert_eq!(state.file, map.source);
    assert_eq!(state.line, 2);
    assert_eq!(
        state.changes[0].contexts,
        vec!["选择：第 9 行🙂（第 4 行）", "第 5 行条件的分支 1"]
    );
}

#[test]
fn equal_runtime_fingerprint_never_authorizes_changed_static_fields_or_locations() {
    for variant in 0..11 {
        let (before, mut after, map) = fixture();
        let catalog = &mut after.analysis.catalog;
        match variant {
            0 => catalog
                .entities
                .get_mut("tower")
                .unwrap()
                .description
                .push('新'),
            1 => catalog
                .entities
                .get_mut("tower")
                .unwrap()
                .display
                .push('新'),
            2 => catalog.entities.get_mut("tower").unwrap().entity_type = "item".into(),
            3 => {
                catalog
                    .entities
                    .get_mut("tower")
                    .unwrap()
                    .properties
                    .insert("height".into(), crate::ast::PropertyValue::Num(39.0));
            }
            4 => {
                catalog
                    .entities
                    .get_mut("tower")
                    .unwrap()
                    .properties
                    .insert("height".into(), crate::ast::PropertyValue::Str("38".into()));
            }
            5 => catalog.entities.get_mut("tower").unwrap().line += 1,
            6 => catalog.states.get_mut("lamp").unwrap().target.id = "keeper".into(),
            7 => catalog.references[0].target.id = "keeper".into(),
            8 => {
                catalog.references.pop();
            }
            9 => {
                catalog.references.push(catalog.references[0].clone());
            }
            _ => catalog.states.get_mut("lamp").unwrap().changes[0].contexts[0].push('新'),
        }
        assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
        assert!(
            equivalent(&before, &after, &map).is_err(),
            "静态字段篡改 {variant}"
        );
    }
}

#[test]
fn only_reference_multiset_and_object_index_can_reorder() {
    let (before, mut after, map) = fixture();
    after.analysis.catalog.references.reverse();
    after.analysis.catalog.objects.reverse();
    equivalent(&before, &after, &map).unwrap();
    after.program.files.reverse();
    assert!(equivalent(&before, &after, &map).is_err());
}

#[test]
fn entity_property_ast_and_columns_are_checked_beyond_the_derived_catalog() {
    let (before, mut after, map) = fixture();
    after
        .program
        .entities
        .iter_mut()
        .find(|entity| entity.name == "tower")
        .unwrap()
        .properties[0]
        .loc
        .column += 1;
    assert!(equivalent(&before, &after, &map).is_err());
    assert_eq!(PathBuf::from(&map.source).file_name().unwrap(), "source.wl");
}
