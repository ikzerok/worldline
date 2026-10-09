use super::*;
use std::fs;
use worldline_core::{
    ast::Stmt,
    evidence_source::{
        resolve_evidence_source, resolve_evidence_sources, EvidenceSource, EvidenceSourceOwner,
        RuntimeOutputSourceIndex,
    },
    project::Project,
    TargetRef,
};
use worldline_runtime::{
    generate_playthrough_report, generate_playthrough_report_with_presentation,
    PlaythroughReportOptions, ReplayCancellation,
};

fn files(name: &str, source: &str, files: &[(&str, &str)]) -> Fixture {
    let mut f = fixture(name, "event initial\n  -> END\n", &[]);
    fs::write(f.root.join("world.wl"), source).unwrap();
    for (path, contents) in files {
        let path = f.root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    f.project = Project::open(&f.root).unwrap();
    f
}

#[test]
fn included_text_say_choice_scene_and_fragment_share_real_relative_link_targets() {
    let main = concat!(
        "include \"chapters/target.wl\"\ninclude \"fragments/target.wl\"\n",
        "include \"fragments/definition.wl\"\ncharacter lin as \"林舟\"\n",
        "event start\n  scene harbor\ninclude \"chapters/body.wl\"\n",
    );
    let body = concat!(
        "    Same {rnd(1, 100)} [[file:target.wl|资料]] #wl-localization:body\n",
        "    say lin \"Same [[file:target.wl|资料]]\" #wl-localization:spoken\n",
        "    choice \"Choice [[file:target.wl|资料]] {rnd(1, 100)}\" #wl-localization:choice\n",
        "      call report()\n      -> END\n",
    );
    let mut f = files(
        "included-rendering",
        main,
        &[
            ("chapters/body.wl", body),
            ("chapters/target.wl", "tag harbor\n"),
            (
                "fragments/definition.wl",
                "fragment report()\ninclude \"lines.wl\"\n",
            ),
            (
                "fragments/lines.wl",
                "  Same [[file:target.wl|资料]] #wl-localization:detail\n  return\n",
            ),
            ("fragments/target.wl", "tag ledger\n"),
        ],
    );
    let c = f.project.compile();
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    f.populate(&["body", "spoken", "choice", "detail"]);
    f.translate(
        "body",
        vec![link("l0", "译资料🌙"), text(" "), placeholder("p0")],
    );
    f.translate("spoken", vec![text("台词 "), link("l0", "译资料")]);
    f.translate(
        "choice",
        vec![link("l0", "译选项"), text(" "), placeholder("p0")],
    );
    f.translate("detail", vec![text("片段 "), link("l0", "译资料")]);
    let presentation = f.presentation(Policy::Strict);
    let mut source = Story::new_with_seed(&c.program, &c.analysis, 73).unwrap();
    let mut localized =
        Story::new_with_presentation(&c.program, &c.analysis, 73, &presentation).unwrap();
    let original = source.continue_story().unwrap();
    let translated = localized.continue_story().unwrap();
    assert_eq!(source.state_view(), localized.state_view());
    let target = f
        .root
        .join("chapters/target.wl")
        .to_string_lossy()
        .into_owned();
    for (before, after) in original.iter().zip(&translated) {
        let (
            Output::Text { links: before, .. },
            Output::Text {
                links: after,
                localization: Some(meta),
                ..
            },
        ) = (before, after)
        else {
            panic!("text and say must render")
        };
        assert_eq!(before[0].target.id, target);
        assert_eq!(after[0].target.id, target);
        assert_eq!(meta.source.file, "chapters/body.wl");
        assert_eq!(meta.source_links[0].target.id, target);
    }
    let choice = &localized.choices()[0];
    assert_eq!(source.choices()[0].id, choice.id);
    assert_eq!(source.choices()[0].links[0].target.id, target);
    assert_eq!(choice.links[0].target.id, target);
    assert_eq!(
        choice.localization.as_ref().unwrap().source.file,
        "chapters/body.wl"
    );
    let id = choice.id.clone();
    source.choose_id(&id).unwrap();
    localized.choose_id(&id).unwrap();
    let original = source.continue_story().unwrap();
    let translated = localized.continue_story().unwrap();
    let Output::Text { links: before, .. } = &original[0] else {
        panic!("fragment text")
    };
    let Output::Text {
        links: after,
        localization: Some(meta),
        ..
    } = &translated[0]
    else {
        panic!("fragment translation")
    };
    let target = f
        .root
        .join("fragments/target.wl")
        .to_string_lossy()
        .into_owned();
    assert_eq!(before[0].target.id, target);
    assert_eq!(after[0].target.id, target);
    assert_eq!(meta.source.file, "fragments/lines.wl");
    assert_eq!(source.state_view(), localized.state_view());
    for link in &c.analysis.catalog.text_links {
        let expected = if link.file.ends_with("chapters/body.wl") {
            "chapters/target.wl"
        } else {
            "fragments/target.wl"
        };
        assert_eq!(link.target.id, f.root.join(expected).to_string_lossy());
    }
    let Stmt::Scene(scene) = &c.program.events[0].body[0] else {
        panic!("scene")
    };
    let index = RuntimeOutputSourceIndex::new(&c.program);
    assert!(index
        .get(&scene.body[0])
        .unwrap()
        .ends_with("chapters/body.wl"));
    assert!(
        index.get(&scene.body[0].clone()).is_none(),
        "cloned statements cannot reuse pointer identity"
    );
}

#[test]
fn included_link_diagnostics_use_actual_file_and_do_not_accept_a_root_namesake() {
    let mut f = files(
        "included-link-diagnostics",
        "include \"absent.wl\"\nevent start\ninclude \"chapters/body.wl\"\n",
        &[
            ("absent.wl", "tag root_only\n"),
            (
                "chapters/body.wl",
                "  [[file:absent.wl|不存在的同名目标]]\n  -> END\n",
            ),
        ],
    );
    let c = f.project.compile();
    let diagnostic = c
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "A218")
        .expect("child-relative missing target");
    assert_eq!(
        diagnostic.file,
        f.root.join("chapters/body.wl").to_string_lossy()
    );
    assert!(diagnostic.message.contains("chapters/absent.wl"));
    assert!(c.has_errors());
    assert!(f
        .project
        .prepare_localization_presentation(&request(Policy::SourceFallback))
        .is_err());
}

#[test]
fn ambiguous_included_origins_fail_before_global_evaluation_and_do_not_guess_a_file() {
    let mut f = files(
        "included-ambiguous",
        concat!(
            "include \"target.wl\"\nlet divisor = 0\nlet initial = 1 / divisor\n",
            "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n"
        ),
        &[
            ("target.wl", "tag target\n"),
            (
                "a.wl",
                "  Same [[file:target.wl|资料]] #wl-localization:a\n",
            ),
            (
                "b.wl",
                "  Same [[file:target.wl|资料]] #wl-localization:b\n",
            ),
        ],
    );
    let before = f.project.content_baseline();
    let c = f.project.compile();
    assert!(c.has_errors());
    assert!(c
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "A218")
        .all(|diagnostic| diagnostic.file.is_empty()));
    let index = RuntimeOutputSourceIndex::new(&c.program);
    assert!(index.has_unresolved_file_links());
    assert!(index.get(&c.program.events[0].body[0]).is_none());
    let error = match Story::new_with_seed(&c.program, &c.analysis, 11) {
        Ok(_) => panic!("ambiguous relative links must not start"),
        Err(error) => error,
    };
    assert!(
        error.message.contains("来源"),
        "global division must not run: {}",
        error.message
    );
    assert!(f
        .project
        .prepare_localization_presentation(&request(Policy::SourceFallback))
        .is_err());
    assert_eq!(f.project.content_baseline(), before);
}

#[test]
fn included_choice_evidence_roundtrips_scalar_batch_and_report_in_events_scenes_and_fragments() {
    let mut f = files(
        "included-choice-evidence",
        concat!(
            "include \"fragments/definition.wl\"\nevent start\n",
            "include \"chapters/event.wl\"\n  scene harbor\ninclude \"chapters/scene.wl\"\n",
        ),
        &[
            (
                "chapters/event.wl",
                "  choice \"Enter\" #wl-localization:enter\n    -> start.harbor\n",
            ),
            (
                "chapters/scene.wl",
                "    choice \"Visit\" #wl-localization:visit\n      call report()\n      -> END\n",
            ),
            (
                "fragments/definition.wl",
                "fragment report()\ninclude \"choices.wl\"\n",
            ),
            (
                "fragments/choices.wl",
                "  choice \"Return\" #wl-localization:return\n    return\n",
            ),
        ],
    );
    let c = f.project.compile();
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    f.populate(&["enter", "visit", "return"]);
    for id in ["enter", "visit", "return"] {
        f.translate(id, vec![text(&format!("译 {id}"))]);
    }
    let presentation = f.presentation(Policy::Strict);
    let expected = [
        (
            "chapters/event.wl",
            "start",
            "chapters/scene.wl",
            "start.harbor",
        ),
        (
            "chapters/scene.wl",
            "start.harbor",
            "chapters/event.wl",
            "start",
        ),
        (
            "fragments/choices.wl",
            "fragment:report",
            "chapters/event.wl",
            "start",
        ),
    ];
    for localized in [false, true] {
        let mut story = if localized {
            Story::new_with_presentation(&c.program, &c.analysis, 73, &presentation).unwrap()
        } else {
            Story::new_with_seed(&c.program, &c.analysis, 73).unwrap()
        };
        let mut displayed = Vec::new();
        for (file, node, wrong_file, wrong_node) in expected {
            story.continue_story().unwrap();
            let explanations = story.choice_evidence().unwrap();
            let source = explanations[0]
                .source
                .as_ref()
                .expect("actual choice source");
            assert_eq!(source.file, f.root.join(file).to_string_lossy());
            assert_eq!(source.line, 1);
            assert_eq!(
                source.owner,
                EvidenceSourceOwner::Choice { node: node.into() }
            );
            let target = resolve_evidence_source(&c, source).unwrap();
            assert_eq!(target.path, f.root.join(file));
            assert!(c.sources[&target.path][target.range.clone()].starts_with("choice \""));
            let mut wrong_file_source = source.clone();
            wrong_file_source.file = f.root.join(wrong_file).to_string_lossy().into_owned();
            let mut wrong_node_source = source.clone();
            wrong_node_source.owner = EvidenceSourceOwner::Choice {
                node: wrong_node.into(),
            };
            assert!(resolve_evidence_source(&c, &wrong_file_source).is_err());
            assert!(resolve_evidence_source(&c, &wrong_node_source).is_err());
            let batch = resolve_evidence_sources(
                &c,
                &[source, &wrong_file_source, source, &wrong_node_source],
            )
            .unwrap();
            assert_eq!(batch[0].as_ref().unwrap(), &target);
            assert_eq!(batch[2].as_ref().unwrap(), &target);
            assert!(batch[1].is_err());
            assert!(batch[3].is_err());
            displayed.push(story.choices()[0].label.clone());
            let id = story.choices()[0].id.clone();
            story.choose_id(&id).unwrap();
        }
        story.continue_story().unwrap();
        assert!(story.is_ended());
        let trace = story.replay_trace();
        let report = if localized {
            generate_playthrough_report_with_presentation(
                &c,
                &trace,
                PlaythroughReportOptions::default(),
                &ReplayCancellation::new(),
                &presentation,
            )
        } else {
            generate_playthrough_report(
                &c,
                &trace,
                PlaythroughReportOptions::default(),
                &ReplayCancellation::new(),
            )
        }
        .unwrap();
        let choices = report
            .observations
            .iter()
            .filter_map(|observation| observation.choice.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(choices.len(), expected.len());
        for ((choice, (file, node, _, _)), displayed) in choices.iter().zip(expected).zip(displayed)
        {
            assert_eq!(choice.label, displayed);
            let position = choice.source.as_ref().expect("report source");
            assert_eq!(position.file, file);
            assert_eq!(position.line, 1);
            let source = EvidenceSource {
                file: f.root.join(&position.file).to_string_lossy().into_owned(),
                line: position.line,
                owner: EvidenceSourceOwner::Choice {
                    node: choice.node.clone(),
                },
            };
            assert_eq!(choice.node, node);
            assert_eq!(
                resolve_evidence_source(&c, &source).unwrap().path,
                f.root.join(file)
            );
        }
    }
}

#[test]
fn ambiguous_stable_links_keep_source_execution_and_unlocated_reference_facts() {
    let mut f = files(
        "ambiguous-stable-links",
        "character lin\nevent start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n",
        &[
            ("a.wl", "  [[character:lin|林]]\n"),
            ("b.wl", "  [[character:lin|林]]\n"),
        ],
    );
    let c = f.project.compile();
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    let index = RuntimeOutputSourceIndex::new(&c.program);
    assert!(!index.has_unresolved_file_links());
    assert!(index.get(&c.program.events[0].body[0]).is_none());
    assert!(
        c.analysis.catalog.text_links.is_empty(),
        "no guessed editable source"
    );
    let target = TargetRef::new("character", "lin");
    let references = c.analysis.catalog.references_to(&target);
    assert_eq!(references.len(), 2);
    assert!(references
        .iter()
        .all(|reference| reference.file.is_empty() && reference.line == 0));
    let baseline = f.project.content_baseline();
    let impact = f.project.deletion_impact(&target);
    assert!(!impact.content_references.is_empty());
    assert!(
        !impact.can_delete(),
        "unknown positions cannot erase known references"
    );
    let rename = f
        .project
        .plan_rename_target(&target, "lin_renamed")
        .unwrap_err();
    assert!(rename.contains("源码未载入"), "{rename}");
    assert_eq!(f.project.content_baseline(), baseline);
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 11).unwrap();
    let outputs = story.continue_story().unwrap();
    assert_eq!(outputs.len(), 3);
    assert!(matches!(outputs.last(), Some(Output::Ended)));
    for output in &outputs[..2] {
        let Output::Text {
            content,
            links,
            localization,
            ..
        } = output
        else {
            panic!("text")
        };
        assert_eq!(content, "林");
        assert_eq!(links[0].target, target);
        assert_eq!((links[0].start, links[0].end), (0, "林".len()));
        assert!(localization.is_none());
    }
    assert!(story.is_ended());
    assert!(f
        .project
        .prepare_localization_presentation(&request(Policy::SourceFallback))
        .is_err());
}

#[test]
fn ambiguous_stable_links_still_validate_targets_without_invented_diagnostic_locations() {
    let mut f = files(
        "ambiguous-missing-stable-target",
        "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n  -> END\n",
        &[
            ("a.wl", "  [[character:absent|林]]\n"),
            ("b.wl", "  [[character:absent|林]]\n"),
        ],
    );
    let c = f.project.compile();
    assert!(c.has_errors());
    let errors = c
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == "A218")
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 2);
    assert!(errors.iter().all(|diagnostic| {
        diagnostic.message.contains("character absent")
            && diagnostic.file.is_empty()
            && diagnostic.span.line == 0
            && diagnostic.source_role
                == Some(worldline_core::diagnostic::DiagnosticSourceRole::Unavailable)
    }));
}

#[test]
fn ambiguous_choice_origins_remain_playable_but_evidence_and_report_never_guess() {
    let mut f = files(
        "ambiguous-choice-evidence",
        "event start\ninclude \"a.wl\"\ninclude \"b.wl\"\n",
        &[
            ("a.wl", "  choice \"Left\"\n    -> END\n"),
            ("b.wl", "  choice \"Right\"\n    -> END\n"),
        ],
    );
    let c = f.project.compile();
    assert!(!c.has_errors(), "{:?}", c.diagnostics);
    let mut story = Story::new_with_seed(&c.program, &c.analysis, 11).unwrap();
    story.continue_story().unwrap();
    assert_eq!(story.choices().len(), 2);
    assert!(story
        .choice_evidence()
        .unwrap()
        .iter()
        .all(|choice| choice.source.is_none()));
    let guesses = ["a.wl", "b.wl", "world.wl"].map(|file| EvidenceSource {
        file: f.root.join(file).to_string_lossy().into_owned(),
        line: 1,
        owner: EvidenceSourceOwner::Choice {
            node: "start".into(),
        },
    });
    for guess in &guesses {
        assert!(resolve_evidence_source(&c, guess).is_err());
    }
    assert!(
        resolve_evidence_sources(&c, &guesses.iter().collect::<Vec<_>>())
            .unwrap()
            .iter()
            .all(Result::is_err)
    );
    story.choose(0).unwrap();
    story.continue_story().unwrap();
    let report = generate_playthrough_report(
        &c,
        &story.replay_trace(),
        PlaythroughReportOptions::default(),
        &ReplayCancellation::new(),
    )
    .unwrap();
    assert!(report.observations[1]
        .choice
        .as_ref()
        .unwrap()
        .source
        .is_none());
}
