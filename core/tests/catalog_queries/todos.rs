use super::*;
#[test]
fn todo_projection_groups_missing_targets_and_is_read_only_and_deterministic() {
    static NEXT: AtomicUsize = AtomicUsize::new(2000);
    let root = std::env::temp_dir().join(format!(
        "worldline-catalog-todos-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".world/comments")).unwrap();
    fs::create_dir_all(root.join(".world/proposals")).unwrap();
    let source = concat!(
        "entity keepers kind organization as \"守灯会\"\n",
        "event start\n",
        "  参见 [[entity:missing_place|失落地点]] 与 [[entity:missing_place|另一个失落地点]]。\n",
        "  -> END\n",
    );
    fs::write(root.join("world.wl"), source).unwrap();
    fs::write(
        root.join(".world/project.json"),
        r#"{"schema_version":1,"project_id":"todo_test","language_version":"1.10","entry":"world.wl","required_features":["content.entities.v1","collaboration.comments.v1","collaboration.proposals.v1"],"maps":{},"graph_views":{},"comments":{"detached":".world/comments/detached.json","resolved":".world/comments/resolved.json"},"proposals":{"open":".world/proposals/open.json","accepted":".world/proposals/accepted.json"}}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/comments/detached.json"),
        r#"{"schema_version":1,"id":"detached","author":"甲","body":"检查失落地点","anchor":{"kind":"object","target":{"kind":"entity","id":"missing_place"}},"resolved":false}"#,
    )
    .unwrap();
    fs::write(
        root.join(".world/comments/resolved.json"),
        r#"{"schema_version":1,"id":"resolved","author":"甲","body":"已处理","anchor":{"kind":"object","target":{"kind":"entity","id":"missing_place"}},"resolved":true}"#,
    )
    .unwrap();
    for (id, status) in [("open", "open"), ("accepted", "accepted")] {
        fs::write(
            root.join(format!(".world/proposals/{id}.json")),
            format!(
                r#"{{"schema_version":1,"id":"{id}","author":"甲","reason":"审阅改动","status":"{status}","changes":[{{"path":"world.wl","domain":"content","base":"旧文本","proposed":"新文本"}}]}}"#
            ),
        )
        .unwrap();
    }
    let mut project = Project::open(&root).unwrap();
    let baseline = project.content_baseline();
    let sources = project.sources();
    let fingerprint = project.compile().analysis.fingerprint;

    let first = project.todo_projection();
    let second = project.todo_projection();

    let kinds: Vec<_> = first.items.iter().map(|item| item.kind).collect();
    assert_eq!(
        kinds,
        vec![
            TodoKind::BrokenLink,
            TodoKind::BrokenLink,
            TodoKind::EntryToCreate,
            TodoKind::DetachedComment,
            TodoKind::OpenProposal,
        ]
    );
    assert_eq!(
        first.items[0].target,
        TargetRef::new("entity", "missing_place")
    );
    assert_eq!(
        first.items[1].target,
        TargetRef::new("entity", "missing_place")
    );
    assert!(first.items[2].reason.contains("2 处"));
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        second
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(project.content_baseline(), baseline);
    assert_eq!(project.sources(), sources);
    assert_eq!(project.compile().analysis.fingerprint, fingerprint);
    let _ = fs::remove_dir_all(root);
}
