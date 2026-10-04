use serde_json::{json,Value};
use worldline_core::{catalog_import::CatalogImportRequest,project::Project};
fn exchange(messages:Vec<String>)->Vec<Value> {
    let mut output=Vec::new();
    worldline_agent::run(&mut std::io::Cursor::new(messages.join("\n")),&mut output);
    String::from_utf8(output).unwrap().lines().map(|line|serde_json::from_str(line).unwrap()).collect()
}
#[test]
fn catalog_import_rpc_memory_then_save_and_error_boundaries() {
    let root=std::env::temp_dir().join(format!("catalog-import-rpc-{}",std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let source="character lin as \"林\"\nevent start\n  -> END\n";
    std::fs::write(root.join("world.wl"),source).unwrap();
    let mut project=Project::open(&root).unwrap();
    let request:CatalogImportRequest=serde_json::from_value(json!({"schema_version":1,"expected_baseline":project.content_baseline(),"destination":"world.wl","csv":"kind,id,name\ncharacter,lin,新\n","columns":[{"column":0,"field":{"kind":"kind"}},{"column":1,"field":{"kind":"id"}},{"column":2,"field":{"kind":"display"}}]})).unwrap();
    let plan=project.preview_catalog_import(&request).unwrap();
    let applied=project.apply_catalog_import(&request,&plan.plan_digest).unwrap();
    let msg=|id,method,params|json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string();
    let open=msg(1,"project.open",json!({"path":root}));
    let apply=msg(3,"catalog.import.apply",json!({"project_id":"p1","request":request,"plan_digest":plan.plan_digest}));
    let result=exchange(vec![open.clone(),msg(2,"catalog.import.preview",json!({"project_id":"p1","request":request})),apply.clone()]);
    assert_eq!(result[1]["result"]["plan"],json!(plan));
    assert_eq!(result[2]["result"]["saved"],false);
    assert_eq!(std::fs::read_to_string(root.join("world.wl")).unwrap(),source);
    let result=exchange(vec![open,apply,msg(4,"project.save",json!({"project_id":"p1","expected_baseline":applied.new_baseline})),msg(5,"catalog.import.preview",json!({"project_id":"p1","request":request}))]);
    assert_eq!(result[2]["result"]["saved"],true);
    assert_eq!(result[3]["result"]["ok"],false);
    assert!(result[3].get("error").is_none());
    let duplicate=msg(6,"catalog.import.preview",json!({"project_id":"p1","request":request})).replace("\"schema_version\":1","\"schema_version\":1,\"schema_version\":1");
    let result=exchange(vec![duplicate]);assert_eq!(result[0]["error"]["code"],-32700);
    std::fs::remove_dir_all(root).unwrap();
}
