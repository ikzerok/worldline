//! 动态变更保留表达式，静态无法确定的目标不伪装成已知状态或调用方事件。
use super::StateChangeSite;
use crate::{ast::Expr, catalog::Catalog, language::DynamicChangeStmt};
pub(super) fn add_dynamic(
    change: &DynamicChangeStmt,
    mut site: StateChangeSite,
    catalog: &mut Catalog,
) {
    site.kind = change.kind;
    site.line = change.loc.line;
    site.state_expression = Some(crate::relation_context::expression(&change.state));
    site.tags_expression = Some(crate::relation_context::expression(&change.tags));
    site.tags = static_tags(&change.tags).unwrap_or_default();
    if let Expr::Call { name, args, .. } = &change.state {
        if name == "state" {
            if let Some(id) = args.first().and_then(crate::language::static_id) {
                if let Some(state) = catalog.states.get_mut(id) {
                    state.changes.push(site.clone());
                }
            }
        }
    }
    catalog.dynamic_state_changes.push(site);
}
fn static_tags(expr: &Expr) -> Option<Vec<String>> {
    let Expr::Call { name, args, .. } = expr else {
        return None;
    };
    if name != "tags" {
        return None;
    }
    let mut tags = Vec::new();
    for arg in args {
        let Expr::Call { name, args, .. } = arg else {
            return None;
        };
        if name != "tag" {
            return None;
        }
        tags.push(crate::language::static_id(args.first()?)?.into());
    }
    tags.sort();
    tags.dedup();
    Some(tags)
}
