use worldline_core::{compile_source_with_options, CompileOptions};
use worldline_runtime::Story;
const SOURCE: &str = "character lin as \"林舟\"\ncharacter mei as \"梅\"\nentity boat kind ship\n  property captain = ref(\"character\", \"lin\")\nevent start with lin\n  原文\n  choice \"继续\"\n    -> END\n";
fn options() -> CompileOptions {
    CompileOptions::v1_13()
        .with_object_refs(true)
        .with_character_refs(true)
}

#[test]
fn static_character_ref_retarget_preserves_save_but_person_id_rename_rejects_it() {
    let before = compile_source_with_options("world.wl", SOURCE, options());
    let after = compile_source_with_options(
        "world.wl",
        &SOURCE.replace(
            "captain = ref(\"character\", \"lin\")",
            "captain = ref(\"character\", \"mei\")",
        ),
        options(),
    );
    assert!(!before.has_errors());
    assert!(!after.has_errors());
    assert_eq!(before.analysis.fingerprint, after.analysis.fingerprint);
    let mut story = Story::new_with_seed(&before.program, &before.analysis, 42).unwrap();
    story.continue_story().unwrap();
    let save = story.save().unwrap();
    let mut restored = Story::load(&after.program, &after.analysis, &save).unwrap();
    assert_eq!(restored.save().unwrap(), save);
    restored.choose(0).unwrap();
    restored.continue_story().unwrap();
    assert!(restored.is_ended());
    let renamed =
        compile_source_with_options("world.wl", &SOURCE.replace("lin", "navigator"), options());
    assert!(!renamed.has_errors());
    assert_ne!(before.analysis.fingerprint, renamed.analysis.fingerprint);
    assert!(Story::load(&renamed.program, &renamed.analysis, &save).is_err());
}

#[test]
fn old_112_static_refs_keep_runtime_fingerprint_and_save_when_113_is_explicit() {
    let source = SOURCE.replace("ref(\"character\", \"lin\")", "ref(\"entity\", \"boat\")");
    let old = compile_source_with_options(
        "world.wl",
        &source,
        CompileOptions::v1_12().with_object_refs(true),
    );
    let current = compile_source_with_options(
        "world.wl",
        &source,
        CompileOptions::v1_13().with_object_refs(true),
    );
    let character = compile_source_with_options("world.wl", SOURCE, options());
    assert!(!old.has_errors());
    assert!(!current.has_errors());
    assert_eq!(old.analysis.fingerprint, current.analysis.fingerprint);
    assert_eq!(old.analysis.fingerprint, character.analysis.fingerprint);
    let mut story = Story::new_with_seed(&old.program, &old.analysis, 42).unwrap();
    story.continue_story().unwrap();
    let save = story.save().unwrap();
    for result in [&current, &character] {
        let restored = Story::load(&result.program, &result.analysis, &save).unwrap();
        assert_eq!(restored.save().unwrap(), save);
    }
}
