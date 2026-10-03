use super::*;
use crate::{ast::PropertyValue, catalog::EntityInfo, navigation::TextLinkInfo};
use std::collections::BTreeMap;

fn reference(source: &str, target: &str) -> ReferenceInfo {
    ReferenceInfo {
        source: TargetRef::new("file", source),
        target: TargetRef::new("file", target),
        kind: "正文链接".into(),
        file: source.into(),
        line: 3,
    }
}

#[test]
fn mapped_reference_multiset_is_stable_and_keeps_every_field_and_duplicate() {
    let mut before = Catalog::default();
    let duplicate = reference("a.wl", "a.wl");
    before.references = vec![duplicate.clone(), reference("b.wl", "a.wl"), duplicate];
    let mut after = before.clone();
    for entry in &mut after.references {
        for value in [&mut entry.source.id, &mut entry.target.id, &mut entry.file] {
            if value == "a.wl" {
                *value = "z.wl".into();
            }
        }
    }
    after.references.reverse();
    let expected = normalized_catalog(&before, "a.wl", "z.wl").unwrap();
    assert_eq!(
        expected,
        normalized_catalog(&after, "z.wl", "z.wl").unwrap()
    );
    assert_eq!(expected["references"].as_array().unwrap().len(), 3);
    let mut missing = after.clone();
    missing.references.pop();
    assert_ne!(
        expected,
        normalized_catalog(&missing, "z.wl", "z.wl").unwrap()
    );
    for field in 0..7 {
        let mut changed = after.clone();
        let entry = &mut changed.references[0];
        match field {
            0 => entry.source.kind = "event".into(),
            1 => entry.source.id = "different".into(),
            2 => entry.target.kind = "event".into(),
            3 => entry.target.id = "different".into(),
            4 => entry.kind = "其他引用".into(),
            5 => entry.file = "different.wl".into(),
            _ => entry.line += 1,
        }
        assert_ne!(
            expected,
            normalized_catalog(&changed, "z.wl", "z.wl").unwrap(),
            "field {field}"
        );
    }
    assert_eq!(before.references[0].file, "a.wl");
}

#[test]
fn normalization_preserves_ordered_text_links_and_author_values() {
    let mut before = Catalog::default();
    before.entities.insert(
        "e".into(),
        EntityInfo {
            id: "e".into(),
            entity_type: "place".into(),
            display: "a.wl".into(),
            description: "a.wl".into(),
            file: "a.wl".into(),
            line: 1,
            properties: BTreeMap::from([("file".into(), PropertyValue::Str("a.wl".into()))]),
        },
    );
    before.text_links = ["a", "b"]
        .into_iter()
        .map(|id| TextLinkInfo {
            source: TargetRef::new("event", "entry"),
            target: TargetRef::new("entity", id),
            label: id.into(),
            file: "a.wl".into(),
            line: 2,
            column: 5,
        })
        .collect();
    let value = normalized_catalog(&before, "a.wl", "z.wl").unwrap();
    assert_eq!(value["entities"]["e"]["file"], "z.wl");
    assert_eq!(value["entities"]["e"]["display"], "a.wl");
    assert_eq!(value["entities"]["e"]["description"], "a.wl");
    assert_eq!(
        value["entities"]["e"]["properties"],
        serde_json::to_value(&before.entities["e"].properties).unwrap()
    );
    let mut reordered = before.clone();
    reordered.text_links.reverse();
    assert_ne!(
        value,
        normalized_catalog(&reordered, "a.wl", "z.wl").unwrap()
    );
    let mut changed = before;
    changed
        .entities
        .get_mut("e")
        .unwrap()
        .properties
        .insert("file".into(), PropertyValue::Str("z.wl".into()));
    assert_ne!(value, normalized_catalog(&changed, "a.wl", "z.wl").unwrap());
}

#[test]
fn mapped_adjacency_keeps_parallel_and_self_relation_multiplicity() {
    let mut before = Catalog::default();
    before.relation_index.insert(
        TargetRef::new("file", "a.wl"),
        vec!["self".into(), "self".into(), "parallel".into()],
    );
    before
        .relation_index
        .insert(TargetRef::new("file", "b.wl"), vec!["parallel".into()]);
    let mut after = before.clone();
    let values = after
        .relation_index
        .remove(&TargetRef::new("file", "a.wl"))
        .unwrap();
    after
        .relation_index
        .insert(TargetRef::new("file", "z.wl"), values);
    let expected = normalized_catalog(&before, "a.wl", "z.wl").unwrap();
    assert_eq!(
        expected,
        normalized_catalog(&after, "z.wl", "z.wl").unwrap()
    );
    after
        .relation_index
        .get_mut(&TargetRef::new("file", "z.wl"))
        .unwrap()
        .remove(0);
    assert_ne!(
        expected,
        normalized_catalog(&after, "z.wl", "z.wl").unwrap()
    );
}

#[test]
fn proof_itself_rejects_real_target_choice_and_effect_order_changes() {
    let root = std::env::temp_dir().join(format!("wl-proof-order-{}", std::process::id()));
    let mut original = Project::new(&root);
    let entry = original.entry.clone();
    original.documents.retain(|path, _| path == &entry);
    let source = "character a\ncharacter b\ntag ready\nstate mood on character a with ready\nevent entry\n  effect on enter\n    become mood remove ready\n    become mood add ready\n  [[character:a|人]]\n  choice \"甲\"\n    -> END\n  choice \"乙\"\n    -> END\n";
    original.set_text(&entry, source.into()).unwrap();
    let before = original.compile_current();
    assert!(!before.has_errors(), "{:?}", before.diagnostics);
    let variants = [
        (source.replace("[[character:a|", "[[character:b|"), false),
        (
            source
                .replace("choice \"甲\"", "choice \"TEMP\"")
                .replace("choice \"乙\"", "choice \"甲\"")
                .replace("choice \"TEMP\"", "choice \"乙\""),
            true,
        ),
        (
            source.replace(
                "remove ready\n    become mood add ready",
                "add ready\n    become mood remove ready",
            ),
            true,
        ),
    ];
    for (source, runtime_change) in variants {
        let mut candidate = original.clone();
        candidate.set_text(&entry, source).unwrap();
        let after = candidate.compile_current();
        assert!(!after.has_errors(), "{:?}", after.diagnostics);
        assert_eq!(before.program.files, after.program.files);
        assert_eq!(
            before.analysis.fingerprint != after.analysis.fingerprint,
            runtime_change
        );
        let error = equivalent(&original, &candidate, &before, &entry, &entry).unwrap_err();
        assert_eq!(
            error.kind,
            crate::source_lifecycle::SourceLifecycleFailureKind::SemanticChange
        );
        assert!(
            error.message.contains(if runtime_change {
                "指纹"
            } else {
                "正式引用目标"
            }),
            "{error}"
        );
    }
}
