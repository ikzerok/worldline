//! 复用正式表达式类型检查，不运行、不重建工程分析。
use super::*;

pub(crate) fn condition_diagnostics(
    program: &Program,
    symbols: &Symbols,
    expression: &Expr,
    source: &crate::source_provenance::ExpressionSource,
) -> Vec<Diagnostic> {
    let mut sources = BTreeMap::new();
    source.bind(expression, &mut sources);
    let mut context = Ctx {
        program,
        symbols: symbols.clone(),
        diags: Vec::new(),
        node_ids: HashMap::new(),
        graph_nodes: Vec::new(),
        graph_edges: Vec::new(),
        anchors: Vec::new(),
        cur_file: source.file.clone(),
        locals: HashMap::new(),
        in_fragment: false,
        in_rule: false,
        expression_fallback: source
            .span
            .map(|s| Loc::new(s.line, s.column))
            .unwrap_or(Loc::new(1, 1)),
        expression_sources: sources,
        source_owner: None,
        source_file: Some(source.file.clone()),
    };
    context.check_expr(expression, Some(ValueKind::Bool));
    context.diags
}
