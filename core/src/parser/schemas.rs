use super::*;

impl<'a> Parser<'a> {
    pub(super) fn parse_schema_declaration(
        &mut self,
        program: &mut Program,
        keyword: &str,
        source: &str,
        loc: Loc,
        indent: u32,
        file: &str,
    ) {
        if keyword == "bind" {
            if let Some(binding) = crate::schemas::parse::binding(source, file, loc, self.diags) {
                program.schema_bindings.push(binding);
            }
            return;
        }
        if keyword != "schema" {
            crate::schemas::parse::error(file, loc, "field 只能写在 schema 块内", self.diags);
            return;
        }
        let mut schema = crate::schemas::parse::declaration(source, file, loc, self.diags);
        let mut block_indent = None;
        while let Some(line) = self.peek().cloned() {
            if line.indent <= indent || line.file != file {
                break;
            }
            self.next();
            if *block_indent.get_or_insert(line.indent) != line.indent {
                crate::schemas::parse::error(
                    file,
                    Loc::new(line.no, line.indent + 1),
                    "schema 字段必须使用一致缩进",
                    self.diags,
                );
            }
            match line.kind {
                LineKind::Schema112 {
                    keyword,
                    source,
                    loc,
                } if keyword == "field" => {
                    if let Some(field) =
                        crate::schemas::parse::field(&source, file, loc, self.options(), self.diags)
                    {
                        if let Some(schema) = &mut schema {
                            schema.fields.push(field);
                        }
                    }
                }
                _ => crate::schemas::parse::error(
                    file,
                    Loc::new(line.no, line.indent + 1),
                    "schema 块内仅允许 field 声明",
                    self.diags,
                ),
            }
        }
        if let Some(schema) = schema {
            program.schemas.push(schema);
        }
    }
}
