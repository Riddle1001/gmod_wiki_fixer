use glua_parser::{
    LuaAstNode, LuaAstToken, LuaBlock, LuaClosureExpr, LuaExpr, LuaFuncStat, LuaLiteralToken,
    LuaStat, LuaSyntaxNode,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionContract {
    pub parameters: Vec<String>,
    pub variadic: bool,
    pub implicit_self: bool,
    // None means dynamic expansion, unknown control flow, or an unsupported body.
    pub returns: Option<Vec<Vec<Option<String>>>>,
}

pub fn contract(node: &LuaSyntaxNode) -> Option<FunctionContract> {
    let closure = LuaClosureExpr::cast(node.clone())
        .or_else(|| node.children().find_map(LuaClosureExpr::cast))?;
    let mut parameters = Vec::new();
    let mut variadic = false;
    if let Some(params) = closure.get_params_list() {
        for param in params.get_params() {
            if param.is_dots() {
                variadic = true;
            } else if let Some(name) = param.get_name_token() {
                parameters.push(name.get_text().to_string());
            }
        }
    }
    let implicit_self = LuaFuncStat::cast(node.clone())
        .and_then(|func| func.get_func_name())
        .is_some_and(|name| name.get_text().contains(':'));
    let returns = closure.get_block().and_then(|block| {
        let mut flow = block_flow(&block);
        if flow.unknown {
            return None;
        }
        if flow.falls_through {
            flow.shapes.push(Vec::new());
        }
        flow.shapes.sort();
        flow.shapes.dedup();
        Some(flow.shapes)
    });
    Some(FunctionContract {
        parameters,
        variadic,
        implicit_self,
        returns,
    })
}

struct Flow {
    shapes: Vec<Vec<Option<String>>>,
    falls_through: bool,
    unknown: bool,
}

fn block_flow(block: &LuaBlock) -> Flow {
    let mut flow = Flow {
        shapes: Vec::new(),
        falls_through: true,
        unknown: false,
    };
    for stat in block.get_stats() {
        if !flow.falls_through {
            break;
        }
        match stat {
            LuaStat::ReturnStat(stat) => {
                let expressions: Vec<_> = stat.get_expr_list().collect();
                if expressions.last().is_some_and(expands) {
                    flow.unknown = true;
                } else {
                    flow.shapes
                        .push(expressions.iter().map(literal_type).collect());
                }
                flow.falls_through = false;
            }
            LuaStat::IfStat(stat) => {
                let mut branches: Vec<_> = stat
                    .get_block()
                    .map(|block| vec![block_flow(&block)])
                    .unwrap_or_default();
                for clause in stat.get_all_clause() {
                    if let Some(block) = clause.get_block() {
                        branches.push(block_flow(&block));
                    }
                }
                flow.falls_through = stat.get_else_clause().is_none()
                    || branches.iter().any(|branch| branch.falls_through);
                for branch in branches {
                    flow.unknown |= branch.unknown;
                    flow.shapes.extend(branch.shapes);
                }
            }
            LuaStat::DoStat(stat) => {
                if let Some(block) = stat.syntax().children().find_map(LuaBlock::cast) {
                    let nested = block_flow(&block);
                    flow.shapes.extend(nested.shapes);
                    flow.unknown |= nested.unknown;
                    flow.falls_through = nested.falls_through;
                }
            }
            LuaStat::ForStat(_)
            | LuaStat::ForRangeStat(_)
            | LuaStat::WhileStat(_)
            | LuaStat::RepeatStat(_)
            | LuaStat::GotoStat(_) => flow.unknown = true,
            // Never count returns inside nested function declarations or callbacks.
            _ => {}
        }
    }
    flow
}

fn expands(expr: &LuaExpr) -> bool {
    matches!(expr, LuaExpr::CallExpr(_)) || expr.get_text().trim() == "..."
}

fn literal_type(expr: &LuaExpr) -> Option<String> {
    match expr {
        LuaExpr::TableExpr(_) => Some("table".into()),
        LuaExpr::ClosureExpr(_) => Some("function".into()),
        LuaExpr::ParenExpr(paren) => paren.get_expr().and_then(|expr| literal_type(&expr)),
        LuaExpr::LiteralExpr(literal) => {
            match literal.get_literal()? {
                LuaLiteralToken::String(_) => Some("string".into()),
                LuaLiteralToken::Number(_) => Some("number".into()),
                _ => match literal.get_text().trim() {
                    "true" | "false" => Some("boolean".into()),
                    "nil" => None, // nil often represents an intentionally absent optional result
                    _ => None,
                },
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glua_parser::LuaParser;
    fn get(source: &str) -> FunctionContract {
        let tree = LuaParser::parse(source, crate::ast_indexer::parser_config());
        let func = tree
            .get_chunk_node()
            .descendants::<LuaFuncStat>()
            .next()
            .unwrap();
        contract(func.syntax()).unwrap()
    }
    #[test]
    fn method_self_varargs_and_nested_returns_are_distinct() {
        let value = get(
            "function T:Test(x, ...) local f = function() return 1,2,3 end return true, 'x' end",
        );
        assert_eq!(value.parameters, vec!["x"]);
        assert!(value.variadic && value.implicit_self);
        assert_eq!(
            value.returns,
            Some(vec![vec![Some("boolean".into()), Some("string".into())]])
        );
    }
    #[test]
    fn optional_paths_are_recorded_and_expanding_calls_are_unknown() {
        assert_eq!(
            get("function Test(x) if x then return 1 end end").returns,
            Some(vec![vec![], vec![Some("number".into())]])
        );
        assert!(get("function Test() return Other() end").returns.is_none());
        assert!(
            get("function Test() for i=1,3 do return 1 end end")
                .returns
                .is_none()
        );
        assert_eq!(
            get("function Test() return (Other()) end").returns,
            Some(vec![vec![None]])
        );
    }
}
