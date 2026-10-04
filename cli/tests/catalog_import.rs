use serde_json::{json, Value};
use worldline_core::project::Project;
fn invoke(args: Vec<String>) -> (i32,Value) {
    let mut out=Vec::new();
    let code=wl::run(&args,&mut out,&mut std::io::Cursor::new(Vec::new())).unwrap();
    (code,serde_json::from_slice(&out).unwrap())
}
#[test]
fn catalog_import_cli_preview_apply_explicit_save_and_strict_json() {
    let root=std::env::temp_dir().join(format!("catalog-import-cli-{}",std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let source="character lin as \"林\"\nevent start\n  -> END\n";
    std::fs::write(root.join("world.wl"),source).unwrap();
    let project=Project::open(&root).unwrap();
    let dto=json!({"schema_version":1,"expected_baseline":project.content_baseline(),"destination":"world.wl","csv":"kind,id,name\ncharacter,lin,新\n","columns":[{"column":0,"field":{"kind":"kind"}},{"column":1,"field":{"kind":"id"}},{"column":2,"field":{"kind":"display"}}]});
    let base=vec!["catalog-import".to_string(),"preview".into(),root.to_string_lossy().into_owned(),"--request-json".into(),dto.to_string(),"--json".into()];
    let (code,preview)=invoke(base.clone());
    assert_eq!(code,0,"{preview}");
    assert_eq!(std::fs::read_to_string(root.join("world.wl")).unwrap(),source);
    let mut apply=base.clone();apply[1]="apply".into();apply.extend(["--plan-digest".into(),preview["plan"]["plan_digest"].as_str().unwrap().into()]);
    let (code,result)=invoke(apply.clone());
    assert_eq!(code,0,"{result}");assert_eq!(result["saved"],false);
    assert_eq!(std::fs::read_to_string(root.join("world.wl")).unwrap(),source);
    apply.push("--save".into());
    let (code,result)=invoke(apply);assert_eq!(code,0,"{result}");assert_eq!(result["saved"],true);
    assert!(std::fs::read_to_string(root.join("world.wl")).unwrap().contains("新"));
    let mut bad=base;bad[4]=dto.to_string().replacen("{","{\"schema_version\":1,",1);
    let (code,result)=invoke(bad);assert_eq!(code,2);assert_eq!(result["ok"],false);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn catalog_import_cli_refuses_pending_journal_without_recovery() {
    let root=std::env::temp_dir().join(format!("catalog-import-cli-read-only-{}",std::process::id()));
    std::fs::create_dir_all(root.join(".world/.transactions/pending")).unwrap();
    let source="character lin\nevent start\n  -> END\n";
    let after="character lin as \"不应恢复\"\nevent start\n  -> END\n";
    std::fs::write(root.join("world.wl"),source).unwrap();
    let hash=|bytes:&[u8]| {
        let hash=bytes.iter().fold(0xcbf29ce484222325u64,|hash,byte|(hash^u64::from(*byte)).wrapping_mul(0x100000001b3));
        format!("{hash:016x}")
    };
    let journal=json!({"version":1,"status":"prepared","files":[{"path":"world.wl","before":hash(source.as_bytes()),"after":hash(after.as_bytes()),"payload":after.as_bytes()}]}).to_string();
    let journal_path=root.join(".world/.transactions/pending/journal.json");
    std::fs::write(&journal_path,&journal).unwrap();
    let dto=json!({"schema_version":1,"expected_baseline":"unused","destination":"world.wl","csv":"kind,id\n","columns":[{"column":0,"field":{"kind":"kind"}},{"column":1,"field":{"kind":"id"}}]});
    let (code,result)=invoke(vec!["catalog-import".into(),"preview".into(),root.to_string_lossy().into_owned(),"--request-json".into(),dto.to_string(),"--json".into()]);
    assert_eq!(code,1,"{result}");assert_eq!(result["error"]["code"],"WORKSPACE_READ_REJECTED");
    assert_eq!(std::fs::read_to_string(root.join("world.wl")).unwrap(),source);
    assert_eq!(std::fs::read_to_string(journal_path).unwrap(),journal);
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn catalog_import_cli_non_utf8_and_oversized_csv_are_data_failures() {
    let root=std::env::temp_dir().join(format!("catalog-import-cli-input-{}",std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("world.wl"),"event start\n  -> END\n").unwrap();
    let project=Project::open(&root).unwrap();
    let dto=json!({"schema_version":1,"expected_baseline":project.content_baseline(),"destination":"world.wl","csv":"","columns":[]});
    let path=root.join("data.csv");
    for bytes in [vec![255],vec![b'x';worldline_core::catalog_import::MAX_CSV_BYTES+1]] {
        std::fs::write(&path,bytes).unwrap();
        let (code,result)=invoke(vec!["catalog-import".into(),"preview".into(),root.to_string_lossy().into_owned(),"--request-json".into(),dto.to_string(),"--csv".into(),path.to_string_lossy().into_owned(),"--json".into()]);
        assert_eq!(code,1);assert_eq!(result["error"]["code"],"CSV_INPUT_REJECTED");assert_eq!(result["stage"],"input");
    }
    std::fs::remove_dir_all(root).unwrap();
}
