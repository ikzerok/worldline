//! 目录声明的词法入口,与主词法器共享字符串解码与诊断。
use crate::ast::{Loc, WorldDecl};
use crate::catalog::{AssetDecl, CatalogDecl, CatalogLink, TargetRef, TARGET_KINDS};
use crate::{Diagnostic, Span};

pub(crate) fn parse(
    word: &str,
    rest: &str,
    file: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
) -> CatalogDecl {
    let tokens = tokenize(rest, file, line, diags);
    let token = |i: usize| tokens.get(i).map(|t| t.0.as_str()).unwrap_or("");
    let id_valid = |i: usize| {
        !tokens.get(i).is_some_and(|t| t.1)
            && crate::lexer::valid_identifier(token(i))
            && token(i) != "END"
    };
    let loc = Loc::new(line, 1);
    let mut bad = false;
    let decl = match word {
        "alias" => {
            bad = tokens.len() != 4
                || !TARGET_KINDS.contains(&token(0))
                || tokens.first().is_some_and(|t| t.1)
                || token(1).is_empty()
                || token(2) != "as"
                || tokens.get(2).is_some_and(|t| t.1)
                || !tokens
                    .get(3)
                    .is_some_and(|t| t.1 && !t.0.trim().is_empty() && !t.0.contains(['\n', '\r']));
            CatalogDecl::Alias(crate::navigation::AliasInfo {
                target: TargetRef::new(token(0), token(1)),
                name: token(3).into(),
                file: file.into(),
                line,
            })
        }
        "tag" | "anchor_def" => {
            bad = !id_valid(0)
                || !(tokens.len() == 1 || (tokens.len() == 3 && token(1) == "as" && tokens[2].1));
            bad |=
                word == "anchor_def" && (tokens.len() != 3 || tokens.get(1).is_some_and(|t| t.1));
            let declaration = WorldDecl {
                name: token(0).into(),
                display: tokens.get(2).map(|t| t.0.clone()),
                description: String::new(),
                properties: Vec::new(),
                file: file.into(),
                loc,
            };
            if word == "anchor_def" {
                CatalogDecl::Anchor(declaration)
            } else {
                CatalogDecl::Tag(declaration)
            }
        }
        "anchor_link" => {
            bad = tokens.len() != 3
                || !id_valid(0)
                || !crate::anchors::ANCHOR_TARGET_KINDS.contains(&token(1))
                || tokens.get(1).is_some_and(|t| t.1)
                || !id_valid(2);
            CatalogDecl::AnchorLink(crate::anchors::AnchorLink {
                anchor: token(0).into(),
                target: TargetRef::new(token(1), token(2)),
                file: file.into(),
                line,
            })
        }
        "asset" => {
            bad = !id_valid(0)
                || !["image", "audio", "file"].contains(&token(1))
                || !tokens.get(2).is_some_and(|t| t.1 && !t.0.is_empty())
                || !(tokens.len() == 3 || (tokens.len() == 5 && token(3) == "as" && tokens[4].1));
            CatalogDecl::Asset(AssetDecl {
                id: token(0).into(),
                kind: token(1).into(),
                path: token(2).into(),
                display: tokens
                    .get(4)
                    .map(|t| t.0.clone())
                    .unwrap_or_else(|| token(0).into()),
                file: file.into(),
                loc,
            })
        }
        _ => {
            bad |= !TARGET_KINDS.contains(&token(0))
                || token(1).is_empty()
                || token(2) != "with"
                || tokens.len() < 4
                || !(3..tokens.len()).all(id_valid);
            let link = CatalogLink {
                target: TargetRef::new(token(0), token(1)),
                values: tokens.iter().skip(3).map(|t| t.0.clone()).collect(),
                file: file.into(),
                line,
                inline: false,
            };
            if word == "attach" {
                CatalogDecl::Attach(link)
            } else {
                CatalogDecl::Mark(link)
            }
        }
    };
    if bad {
        diags.push(Diagnostic::error(
            "P004",
            file,
            Span::new(line, 1, word.len() as u32),
            format!("`{word}` 声明格式不完整,请参照目录与独立锚点规范"),
        ));
    }
    decl
}

pub(crate) fn tokenize(
    rest: &str,
    file: &str,
    line: u32,
    diags: &mut Vec<Diagnostic>,
) -> Vec<(String, bool)> {
    let chars = rest.chars().collect::<Vec<_>>();
    // 逗号与空白都用作 ID 列表分隔,引号内路径完整保留。
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() || chars[i] == ',' {
            i += 1;
            continue;
        }
        if chars[i] == '"' {
            match crate::lexer::parse_quoted(&chars, i, file, line, diags) {
                Ok((text, end)) => {
                    tokens.push((text, true));
                    i = end;
                }
                Err(_) => break,
            }
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != ',' {
                i += 1;
            }
            tokens.push((chars[start..i].iter().collect::<String>(), false));
        }
    }
    tokens
}
