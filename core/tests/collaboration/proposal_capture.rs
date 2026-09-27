use super::*;

#[test]
fn capture_dirty_proposal_separates_content_and_presentation_and_skips_registry_bookkeeping() {
    let mut project = project("capture");
    let entry = project.entry.clone();
    let map_path = project.root.join(".world/maps/city.json");
    let mut source = project.document(&entry).unwrap().to_string();
    source.push_str("entity c kind place as \"丙\"\n");
    project.set_text(&entry, source).unwrap();
    project
        .set_authoring_document(&map_path, map_json(30, 0, &["a", "b"], true).into_bytes())
        .unwrap();

    let captured =
        collaboration::capture_dirty_proposal(&project, "draft_review", "甲", "内容与版式分开审阅")
            .unwrap();
    assert_eq!(captured.status, ProposalStatus::Open);
    assert_eq!(captured.changes.len(), 2);
    assert!(captured
        .changes
        .iter()
        .any(|change| change.domain == "content" && change.path == "world.wl"));
    assert!(captured.changes.iter().any(|change| {
        change.domain == "presentation" && change.path == ".world/maps/city.json"
    }));
    assert!(!captured
        .changes
        .iter()
        .any(|change| change.path == ".world/project.json"));
}
