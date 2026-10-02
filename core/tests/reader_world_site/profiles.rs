use super::*;

#[test]
fn route_completion_matches_linear_semantics_and_preserves_public_bytes() {
    let fixture = Fixture::new("route-identity-index", SOURCE, "1.10");
    let manifest_path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("presentation.manuscripts.v1"));
    manifest["manuscripts"] = serde_json::json!({
        "alpha":".world/alpha.json", "beta":".world/beta.json"
    });
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    for (book, event) in [("alpha", "opening"), ("beta", "arrival")] {
        fs::write(
            fixture.root.join(format!(".world/{book}.json")),
            serde_json::to_vec(&serde_json::json!({
                "schema_version":1,"id":book,"title":book,"entries":[
                    {"id":"same","kind":"chapter","title":"同名章节",
                     "target_ref":{"kind":"event","id":event}}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
    }
    let project = fixture.project();
    let before = project.export_files().unwrap();
    for version in [1, 2, 3] {
        let mut choice = selection();
        choice.schema_version = version;
        if version < 3 {
            choice.required_features = if version == 2 {
                vec![READER_FIELDS_FEATURE.into()]
            } else {
                choice.fields.clear();
                Vec::new()
            };
        }
        choice.manuscripts = ["beta", "alpha"]
            .into_iter()
            .map(|book| ReaderManuscriptSelection {
                id: book.into(),
                chapters: vec!["same".into()],
            })
            .collect();
        let object = |kind: &str, id: &str, path: &str| ReaderProfileRoute {
            target: Some(TargetRef::new(kind, id)),
            manuscript_id: None,
            chapter_id: None,
            output_path: path.into(),
        };
        let chapter = |book: &str, id: &str, path: &str| ReaderProfileRoute {
            target: None,
            manuscript_id: Some(book.into()),
            chapter_id: Some(id.into()),
            output_path: path.into(),
        };
        let profile = ReaderPublicationProfile {
            schema_version: READER_PROFILE_SCHEMA_VERSION,
            required_features: vec![READER_PROFILES_FEATURE.into()],
            id: "identity_index".into(),
            title: choice.site_title.clone(),
            selection: choice,
            routes: vec![
                object("asset", "picture", "assets/a0087.png"),
                chapter("alpha", "later", "manuscripts/m0077-c0044.html"),
                object("entity", "mei", "objects/o0888.html"),
                object("entity", "harbor", "objects/o0876.html"),
                chapter("alpha", "same", "manuscripts/m0077-c0033.html"),
            ],
        };
        let preview = project.preview_reader_profile(&profile).unwrap();
        let original_files = project
            .build_reader_profile(&profile, &preview.plan_digest)
            .unwrap();
        let mut linear = profile.clone();
        for entry in &preview.included {
            if !linear.routes.iter().any(|old| {
                old.target == entry.target
                    && old.manuscript_id == entry.manuscript_id
                    && old.chapter_id == entry.chapter_id
            }) {
                linear.routes.push(ReaderProfileRoute {
                    target: entry.target.clone(),
                    manuscript_id: entry.manuscript_id.clone(),
                    chapter_id: entry.chapter_id.clone(),
                    output_path: entry.output_path.clone(),
                });
            }
        }
        let plan = project.preview_save_reader_profile(&profile).unwrap();
        assert_eq!(plan.profile, linear, "v{version} 路由补全改变线性语义");
        assert_eq!(
            &plan.profile.routes[..profile.routes.len()],
            &profile.routes
        );
        assert_eq!(plan, project.preview_save_reader_profile(&linear).unwrap());
        let updated = project.preview_reader_profile(&plan.profile).unwrap();
        assert_eq!(updated, preview, "v{version} 预览或digest改变");
        assert_eq!(
            project
                .build_reader_profile(&plan.profile, &updated.plan_digest)
                .unwrap(),
            original_files
        );
        let mut candidate = project.clone();
        let mut tampered = plan.clone();
        tampered.profile.routes[0].output_path = "assets/a0088.png".into();
        assert!(candidate.apply_save_reader_profile(&tampered).is_err());
        assert_eq!(candidate.export_files().unwrap(), before);
        candidate.apply_save_reader_profile(&plan).unwrap();
        assert_eq!(candidate.reader_profiles().unwrap(), vec![linear]);
    }
    assert_eq!(project.export_files().unwrap(), before);
}

#[test]
fn profile_save_reopen_migrate_preserves_routes_and_unknown_fields() {
    let fixture = Fixture::new("profile", SOURCE, "1.10");
    let mut project = fixture.project();
    let mut choice = selection();
    choice.schema_version = 2;
    choice.required_features = vec![READER_FIELDS_FEATURE.into()];
    let profile = project.create_reader_profile("public", &choice).unwrap();
    let original_routes = profile.routes.clone();
    let plan = project.preview_save_reader_profile(&profile).unwrap();
    let mut tampered = plan.clone();
    tampered.document_path = "outside.json".into();
    assert!(project.apply_save_reader_profile(&tampered).is_err());
    project.apply_save_reader_profile(&plan).unwrap();
    assert!(project.is_dirty());
    project.save().unwrap();
    let profile_path = project.reader_profile_paths()["public"].clone();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(&profile_path).unwrap()).unwrap();
    value["future_extra"] = serde_json::json!({"preserved":true});
    fs::write(&profile_path, serde_json::to_vec(&value).unwrap()).unwrap();
    let mut project = fixture.project();
    let profile = project.reader_profiles().unwrap().remove(0);
    assert_eq!(profile.routes, original_routes);
    let migration = project.preview_reader_profile_migration(&profile).unwrap();
    assert!(!migration.authorization_changes.is_empty());
    assert_eq!(migration.after.selection.schema_version, 3);
    assert!(!migration
        .after
        .selection
        .required_features
        .contains(&READER_STORY_FEATURE.into()));
    assert_eq!(migration.after.selection.fields, profile.selection.fields);
    assert_eq!(
        migration.after.selection.attachments,
        profile.selection.attachments
    );
    assert_eq!(migration.after.routes, original_routes);
    let candidate = project.apply_reader_profile_migration(&migration).unwrap();
    let plan = project.preview_save_reader_profile(&candidate).unwrap();
    project.apply_save_reader_profile(&plan).unwrap();
    assert!(project.apply_save_reader_profile(&plan).is_err());
    project.save().unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(&profile_path).unwrap()).unwrap();
    assert_eq!(value["future_extra"]["preserved"], true);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(fixture.root.join(".world/project.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["future_key"]["keep"], "preserve");
    assert_eq!(manifest["language_version"], "1.10");
}

#[test]
fn saved_profile_refactor_keeps_public_routes_and_full_backup() {
    let fixture = Fixture::new("refactor", SOURCE, "1.10");
    let mut project = fixture.project();
    let profile = project
        .create_reader_profile("public", &selection())
        .unwrap();
    let old_route = profile
        .routes
        .iter()
        .find(|r| r.target.as_ref() == Some(&TargetRef::new("entity", "harbor")))
        .unwrap()
        .output_path
        .clone();
    let save = project.preview_save_reader_profile(&profile).unwrap();
    project.apply_save_reader_profile(&save).unwrap();
    project.save().unwrap();
    let rename = project
        .plan_rename_target(&TargetRef::new("entity", "harbor"), "harbor_new")
        .unwrap();
    project.apply_rename_plan(&rename).unwrap();
    let changed = project.reader_profiles().unwrap().remove(0);
    let preview = project.preview_reader_profile(&changed).unwrap();
    assert_eq!(route(&preview, "entity", "harbor_new"), old_route);
    assert!(changed
        .selection
        .objects
        .contains(&TargetRef::new("entity", "harbor_new")));
    let backup = project.export_files().unwrap();
    assert_eq!(
        backup[Path::new("unreferenced.txt")],
        b"CANARY_UNREFERENCED_BACKUP"
    );
    assert!(backup.contains_key(Path::new(".world/reader-profiles/public.json")));
}

#[test]
fn cancellation_and_publish_race_never_leave_or_overwrite_a_target() {
    let fixture = Fixture::new("atomic", SOURCE, "1.10");
    let output = Fixture::new("atomic-output", "", "1.10");
    let project = fixture.project();
    let choice = selection();
    let preview = project.preview_reader_export(&choice).unwrap();
    let mut original: Vec<_> = fs::read_dir(&output.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    original.sort();
    for phase in ["render", "write"] {
        let target = output.root.join(phase);
        let result = project.export_reader_site_with_progress(
            &choice,
            &preview.plan_digest,
            &target,
            &mut |progress| progress.phase != phase,
        );
        assert!(result.unwrap_err().starts_with("READER_CANCELLED"));
        assert!(!target.exists());
    }
    let mut remaining: Vec<_> = fs::read_dir(&output.root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    remaining.sort();
    assert_eq!(remaining, original);
    let existing = output.root.join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("keep.txt"), "preserved").unwrap();
    assert!(project
        .export_reader_site(&choice, &preview.plan_digest, &existing)
        .is_err());
    assert_eq!(
        fs::read_to_string(existing.join("keep.txt")).unwrap(),
        "preserved"
    );
    assert!(project
        .export_reader_site(&choice, &preview.plan_digest, &fixture.root.join("inside"))
        .is_err());
    let race = output.root.join("race");
    let mut raced = false;
    let result = project.export_reader_site_with_progress(
        &choice,
        &preview.plan_digest,
        &race,
        &mut |progress| {
            if progress.phase == "publish" {
                fs::create_dir(&race).unwrap();
                raced = true;
            }
            true
        },
    );
    assert!(raced && result.is_err());
    assert!(race.is_dir());
    assert_eq!(fs::read_dir(&race).unwrap().count(), 0);
    assert!(!project.is_dirty());
}

#[test]
fn manuscript_body_order_and_profile_routes_are_preserved() {
    let fixture = Fixture::new("chapters", SOURCE, "1.10");
    let manifest_path = fixture.root.join(".world/project.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["required_features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("presentation.manuscripts.v1"));
    manifest["manuscripts"] = serde_json::json!({"book":".world/book.json"});
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    fs::write(fixture.root.join(".world/book.json"), serde_json::to_vec(&serde_json::json!({
        "schema_version":1,"id":"book","title":"旅途","entries":[
            {"id":"z_first","kind":"chapter","title":"第一章","target_ref":{"kind":"event","id":"opening"}},
            {"id":"a_second","kind":"chapter","title":"第二章","target_ref":{"kind":"event","id":"arrival"}}
        ]
    })).unwrap()).unwrap();
    let project = fixture.project();
    let mut choice = selection();
    choice.manuscripts.push(ReaderManuscriptSelection {
        id: "book".into(),
        chapters: vec!["z_first".into(), "a_second".into()],
    });
    let profile = project
        .create_reader_profile("book_public", &choice)
        .unwrap();
    let preview = project.preview_reader_profile(&profile).unwrap();
    let files = project
        .build_reader_profile(&profile, &preview.plan_digest)
        .unwrap();
    let chapters: Vec<_> = preview
        .included
        .iter()
        .filter(|entry| entry.manuscript_id.is_some())
        .collect();
    assert_eq!(chapters[0].chapter_id.as_deref(), Some("z_first"));
    let first = String::from_utf8_lossy(&files[Path::new(&chapters[0].output_path)]);
    assert!(first.contains("Begin the journey") && first.contains("下一章"));
    assert!(!first.contains("上一章"));
    let second = String::from_utf8_lossy(&files[Path::new(&chapters[1].output_path)]);
    assert!(second.contains("上一章") && second.contains("旅程结束"));
    assert_eq!(
        profile.selection.manuscripts[0].chapters,
        choice.manuscripts[0].chapters
    );
    let mut legacy_choice = choice.clone();
    legacy_choice.schema_version = 2;
    legacy_choice.required_features = vec![READER_FIELDS_FEATURE.into()];
    legacy_choice.manuscripts[0].chapters = vec!["z_first".into()];
    let mut legacy = project
        .create_reader_profile("legacy_book", &legacy_choice)
        .unwrap();
    legacy.selection.manuscripts[0]
        .chapters
        .insert(0, "a_second".into());
    let preview = project.preview_reader_profile(&legacy).unwrap();
    let chapter = |id: &str| {
        preview
            .included
            .iter()
            .find(|entry| entry.chapter_id.as_deref() == Some(id))
            .unwrap()
            .output_path
            .as_str()
    };
    assert_eq!(chapter("z_first"), "manuscripts/m0001-c0001.html");
    assert_eq!(chapter("a_second"), "manuscripts/m0001-c0002.html");
}

#[test]
fn old_profiles_allocate_new_ordinals_without_reusing_reserved_routes() {
    let fixture = Fixture::new("legacy-route-growth", SOURCE, "1.10");
    let project = fixture.project();
    let mut choice = selection();
    choice.schema_version = 2;
    choice.required_features = vec![READER_FIELDS_FEATURE.into()];
    choice.objects = vec![TargetRef::new("entity", "lighthouse")];
    choice.fields.clear();
    choice.attachments.clear();
    let mut profile = project.create_reader_profile("legacy", &choice).unwrap();
    assert_eq!(profile.routes[0].output_path, "objects/o0001.html");
    profile
        .selection
        .objects
        .insert(0, TargetRef::new("entity", "harbor"));
    let preview = project.preview_reader_profile(&profile).unwrap();
    assert_eq!(
        route(&preview, "entity", "lighthouse"),
        "objects/o0001.html"
    );
    assert_eq!(route(&preview, "entity", "harbor"), "objects/o0002.html");
    let mut expanded = project
        .preview_save_reader_profile(&profile)
        .unwrap()
        .profile;
    expanded.selection.objects = vec![TargetRef::new("entity", "vault")];
    let preview = project.preview_reader_profile(&expanded).unwrap();
    assert_eq!(route(&preview, "entity", "vault"), "objects/o0003.html");
    assert_eq!(
        expanded.routes,
        project
            .preview_save_reader_profile(&profile)
            .unwrap()
            .profile
            .routes
    );
}
