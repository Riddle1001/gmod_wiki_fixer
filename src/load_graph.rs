use crate::ast_indexer::SymbolDefinition;
use crate::audit_checks::SourceEvidence;
use glua_parser::{
    LuaAstNode, LuaAstToken, LuaBlock, LuaCallExpr, LuaExpr, LuaLiteralToken, LuaParser, LuaStat,
    LuaSyntaxNode,
};
use std::collections::{HashMap, HashSet, VecDeque};

pub const SERVER: u8 = 1;
pub const CLIENT: u8 = 2;
pub const MENU: u8 = 4;

#[derive(Default)]
pub struct LoadGraph {
    active: HashMap<(String, u8), HashSet<usize>>,
    traces: HashMap<(String, u8), Vec<SourceEvidence>>,
    pub unresolved: HashSet<(String, usize, String)>,
}

pub fn realm_names(mask: u8) -> String {
    [(SERVER, "Server"), (CLIENT, "Client"), (MENU, "Menu")]
        .into_iter()
        .filter_map(|(realm, name)| (mask & realm != 0).then_some(name))
        .collect::<Vec<_>>()
        .join(" and ")
}

impl LoadGraph {
    pub fn build(sources: &HashMap<String, String>) -> Self {
        let mut graph = Self::default();
        let lookup: HashMap<_, _> = sources
            .keys()
            .map(|path| (normalize(path), path.clone()))
            .collect();
        let mut paths: Vec<_> = sources.keys().cloned().collect();
        paths.sort();
        let mut queue = VecDeque::new();
        for path in paths {
            let mask = engine_entry(&path);
            for realm in [SERVER, CLIENT, MENU] {
                if mask & realm != 0 {
                    queue.push_back((
                        path.clone(),
                        realm,
                        vec![SourceEvidence {
                            file: path.clone(),
                            start: 1,
                            end: 1,
                            label: format!("{} engine entrypoint", realm_names(realm)),
                        }],
                    ));
                }
            }
        }
        while let Some((path, realm, trace)) = queue.pop_front() {
            if graph.active.contains_key(&(path.clone(), realm)) {
                continue;
            }
            let source = &sources[&path];
            let tree = LuaParser::parse(source, crate::ast_indexer::parser_config());
            let mut walker = Walker {
                realm,
                file: &path,
                source,
                active: HashSet::new(),
                edges: Vec::new(),
                unresolved: Vec::new(),
                locals: vec![HashSet::new()],
                mutated: HashSet::new(),
            };
            if let Some(block) = tree.get_chunk_node().get_block() {
                walker.block(&block, false);
            }
            graph.active.insert((path.clone(), realm), walker.active);
            graph.traces.insert((path.clone(), realm), trace.clone());
            graph.unresolved.extend(walker.unresolved);
            for (target, line, label, module) in walker.edges {
                let candidates = if module {
                    vec![format!("lua/includes/modules/{}.lua", target)]
                } else {
                    let parent = path
                        .rsplit_once('/')
                        .map(|(parent, _)| parent)
                        .unwrap_or("");
                    vec![
                        format!("{}/{}", parent, target),
                        format!("lua/{}", target),
                        target.clone(),
                    ]
                };
                if let Some(target_path) = candidates
                    .iter()
                    .find_map(|candidate| lookup.get(&normalize(candidate)))
                {
                    let mut child_trace = trace.clone();
                    child_trace.push(SourceEvidence {
                        file: path.clone(),
                        start: line,
                        end: line,
                        label: format!("{} loads {}", label, target_path),
                    });
                    queue.push_back((target_path.clone(), realm, child_trace));
                } else {
                    // require may legitimately resolve an engine/binary module.
                    if !module {
                        graph.unresolved.insert((
                            path.clone(),
                            line,
                            format!("unresolved literal load {}", target),
                        ));
                    }
                }
            }
        }
        graph
    }

    pub fn symbol_realms(&self, file: &str, def: &SymbolDefinition) -> u8 {
        [SERVER, CLIENT, MENU]
            .into_iter()
            .filter(|realm| {
                self.active
                    .get(&(file.to_string(), *realm))
                    .is_some_and(|offsets| {
                        offsets.contains(&def.definition_offset)
                            && def
                                .registration_offset
                                .is_none_or(|offset| offsets.contains(&offset))
                    })
            })
            .fold(0, |mask, realm| mask | realm)
    }

    pub fn evidence(&self, file: &str, mask: u8) -> Vec<SourceEvidence> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for realm in [SERVER, CLIENT, MENU] {
            if mask & realm == 0 {
                continue;
            }
            if let Some(trace) = self.traces.get(&(file.to_string(), realm)) {
                for source in trace {
                    if seen.insert((source.file.clone(), source.start, source.label.clone())) {
                        result.push(source.clone());
                    }
                }
            }
        }
        result
    }

    pub fn traced_files(&self) -> usize {
        self.active
            .keys()
            .map(|(path, _)| path)
            .collect::<HashSet<_>>()
            .len()
    }
}

fn engine_entry(path: &str) -> u8 {
    let path = normalize(path);
    match path.as_str() {
        "lua/includes/init.lua" => SERVER | CLIENT,
        "lua/includes/init_menu.lua" | "lua/menu/menu.lua" => MENU,
        "lua/derma/init.lua" => CLIENT | MENU,
        "lua/skins/default.lua" => CLIENT,
        _ => {
            let parts: Vec<_> = path.split('/').collect();
            match parts.as_slice() {
                ["lua", "vgui", _] => CLIENT | MENU,
                ["lua", "postprocess" | "matproxy", _] => CLIENT,
                ["lua", "autorun", _] => SERVER | CLIENT,
                ["lua", "autorun", "client", _] => CLIENT,
                ["lua", "autorun", "server", _] => SERVER,
                ["gamemodes", _, "gamemode", "init.lua"] => SERVER,
                ["gamemodes", _, "gamemode", "cl_init.lua"] => CLIENT,
                ["lua", "entities" | "weapons", _] => SERVER | CLIENT,
                ["lua", "entities" | "weapons", _, "init.lua"] => SERVER,
                ["lua", "entities" | "weapons", _, "cl_init.lua"] => CLIENT,
                _ => 0,
            }
        }
    }
}

fn normalize(path: &str) -> String {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let mut parts = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

struct Walker<'a> {
    realm: u8,
    file: &'a str,
    source: &'a str,
    active: HashSet<usize>,
    edges: Vec<(String, usize, String, bool)>,
    unresolved: Vec<(String, usize, String)>,
    locals: Vec<HashSet<String>>,
    mutated: HashSet<String>,
}

impl Walker<'_> {
    fn shadowed(&self, name: &str) -> bool {
        self.mutated.contains(name) || self.locals.iter().any(|scope| scope.contains(name))
    }
    fn boolean(&self, expr: &LuaExpr) -> Option<bool> {
        match expr {
            LuaExpr::NameExpr(name) => {
                let name = name.get_text();
                if self.shadowed(&name) {
                    return None;
                }
                match name.as_str() {
                    "SERVER" | "GAME_DLL" => Some(self.realm == SERVER),
                    "CLIENT" => Some(self.realm == CLIENT || self.realm == MENU),
                    "CLIENT_DLL" => Some(self.realm == CLIENT),
                    "MENU_DLL" => Some(self.realm == MENU),
                    _ => None,
                }
            }
            LuaExpr::LiteralExpr(literal) => match literal.get_text().as_str() {
                "true" => Some(true),
                "false" | "nil" => Some(false),
                _ => None,
            },
            LuaExpr::ParenExpr(paren) => paren.get_expr().and_then(|expr| self.boolean(&expr)),
            LuaExpr::UnaryExpr(unary) => {
                if unary
                    .get_op_token()
                    .is_some_and(|op| matches!(op.get_text(), "not" | "!"))
                {
                    unary
                        .syntax()
                        .children()
                        .find_map(LuaExpr::cast)
                        .and_then(|expr| self.boolean(&expr))
                        .map(|value| !value)
                } else {
                    None
                }
            }
            LuaExpr::BinaryExpr(binary) => {
                let values: Vec<_> = binary
                    .syntax()
                    .children()
                    .filter_map(LuaExpr::cast)
                    .map(|expr| self.boolean(&expr))
                    .collect();
                let (Some(left), Some(right)) = (values.first(), values.get(1)) else {
                    return None;
                };
                match binary.get_op_token()?.get_text() {
                    "and" | "&&" => {
                        if *left == Some(false) || *right == Some(false) {
                            Some(false)
                        } else if *left == Some(true) && *right == Some(true) {
                            Some(true)
                        } else {
                            None
                        }
                    }
                    "or" | "||" => {
                        if *left == Some(true) || *right == Some(true) {
                            Some(true)
                        } else if *left == Some(false) && *right == Some(false) {
                            Some(false)
                        } else {
                            None
                        }
                    }
                    "==" => Some((*left)? == (*right)?),
                    "~=" | "!=" => Some((*left)? != (*right)?),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    fn expr(&mut self, expr: &LuaExpr) {
        self.active
            .insert(expr.syntax().text_range().start().into());
        if matches!(expr, LuaExpr::ClosureExpr(_)) {
            return;
        }
        if let LuaExpr::CallExpr(call) = expr {
            self.call(call);
            return;
        }
        for child in expr.syntax().children() {
            if let Some(expr) = LuaExpr::cast(child.clone()) {
                self.expr(&expr);
            } else {
                self.active.insert(child.text_range().start().into());
                for expr in child.children().filter_map(LuaExpr::cast) {
                    self.expr(&expr);
                }
            }
        }
    }
    fn call(&mut self, call: &LuaCallExpr) {
        self.active
            .insert(call.syntax().text_range().start().into());
        let name = call
            .get_prefix_expr()
            .map(|expr| expr.get_text())
            .unwrap_or_default();
        let args: Vec<_> = call
            .get_args_list()
            .map(|args| args.get_args().collect())
            .unwrap_or_default();
        if matches!(name.as_str(), "include" | "require" | "vgui.RegisterFile")
            && !self.shadowed(name.split('.').next().unwrap())
        {
            let target = args.first().and_then(|arg| {
                let LuaExpr::LiteralExpr(literal) = arg else {
                    return None;
                };
                let LuaLiteralToken::String(token) = literal.get_literal()? else {
                    return None;
                };
                Some(token.get_value())
            });
            let offset: usize = call.syntax().text_range().start().into();
            let line = self.source[..offset]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count()
                + 1;
            if let Some(target) = target {
                self.edges
                    .push((target, line, name.clone(), name == "require"));
            } else {
                self.unresolved
                    .push((self.file.into(), line, format!("dynamic {} target", name)));
            }
        }
        for arg in args {
            self.expr(&arg);
        }
    }
    fn unknown_loads(&mut self, node: &LuaSyntaxNode) {
        for call in node.descendants().filter_map(LuaCallExpr::cast) {
            let name = call
                .get_prefix_expr()
                .map(|expr| expr.get_text())
                .unwrap_or_default();
            if matches!(name.as_str(), "include" | "require" | "vgui.RegisterFile") {
                let offset: usize = call.syntax().text_range().start().into();
                let line = self.source[..offset]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                self.unresolved.push((
                    self.file.into(),
                    line,
                    format!("conditional or deferred {}", name),
                ));
            }
        }
    }
    fn block(&mut self, block: &LuaBlock, scoped: bool) -> bool {
        if scoped {
            self.locals.push(HashSet::new());
        }
        let mut terminated = false;
        for stat in block.get_stats() {
            self.active
                .insert(stat.syntax().text_range().start().into());
            match stat {
                LuaStat::ReturnStat(_) => {
                    terminated = true;
                    break;
                }
                LuaStat::LocalStat(stat) => {
                    for expr in stat.get_value_exprs() {
                        self.expr(&expr);
                    }
                    for name in stat.get_local_name_list() {
                        if let Some(name) = name.get_name_token() {
                            self.locals
                                .last_mut()
                                .unwrap()
                                .insert(name.get_text().to_string());
                        }
                    }
                }
                LuaStat::LocalFuncStat(stat) => {
                    if let Some(name) = stat.get_local_name().and_then(|name| name.get_name_token())
                    {
                        self.locals
                            .last_mut()
                            .unwrap()
                            .insert(name.get_text().to_string());
                    }
                    self.unknown_loads(stat.syntax());
                }
                LuaStat::FuncStat(stat) => self.unknown_loads(stat.syntax()),
                LuaStat::AssignStat(stat) => {
                    let (vars, exprs) = stat.get_var_and_expr_list();
                    for expr in exprs {
                        self.expr(&expr);
                    }
                    for var in vars {
                        let text = var.get_text();
                        if matches!(
                            text.as_str(),
                            "SERVER" | "CLIENT" | "CLIENT_DLL" | "MENU_DLL" | "GAME_DLL"
                        ) {
                            self.mutated.insert(text);
                        }
                    }
                }
                LuaStat::CallExprStat(stat) => {
                    if let Some(call) = stat.get_call_expr() {
                        self.call(&call);
                    }
                }
                LuaStat::IfStat(stat) => {
                    let mut selected = None;
                    let mut unknown = false;
                    match stat
                        .get_condition_expr()
                        .and_then(|expr| self.boolean(&expr))
                    {
                        Some(true) => selected = stat.get_block(),
                        Some(false) => {}
                        None => unknown = true,
                    }
                    if selected.is_none() && !unknown {
                        for clause in stat.get_all_clause() {
                            match clause
                                .get_condition_expr()
                                .map(|expr| self.boolean(&expr))
                                .unwrap_or(Some(true))
                            {
                                Some(true) => {
                                    selected = clause.get_block();
                                    break;
                                }
                                Some(false) => {}
                                None => {
                                    unknown = true;
                                    break;
                                }
                            }
                        }
                    }
                    if unknown {
                        self.unknown_loads(stat.syntax());
                        if may_return(stat.syntax()) {
                            terminated = true;
                            break;
                        }
                    } else if let Some(block) = selected {
                        if self.block(&block, true) {
                            terminated = true;
                            break;
                        }
                    }
                }
                LuaStat::DoStat(stat) => {
                    if let Some(block) = stat.syntax().children().find_map(LuaBlock::cast) {
                        if self.block(&block, true) {
                            terminated = true;
                            break;
                        }
                    }
                }
                LuaStat::ForStat(_)
                | LuaStat::ForRangeStat(_)
                | LuaStat::WhileStat(_)
                | LuaStat::RepeatStat(_) => {
                    self.unknown_loads(stat.syntax());
                    if may_return(stat.syntax()) {
                        terminated = true;
                        break;
                    }
                }
                LuaStat::GotoStat(_) => {
                    terminated = true;
                    break;
                }
                _ => {}
            }
        }
        if scoped {
            self.locals.pop();
        }
        terminated
    }
}

fn may_return(node: &LuaSyntaxNode) -> bool {
    if glua_parser::LuaClosureExpr::cast(node.clone()).is_some()
        || glua_parser::LuaFuncStat::cast(node.clone()).is_some()
        || glua_parser::LuaLocalFuncStat::cast(node.clone()).is_some()
    {
        return false;
    }
    if glua_parser::LuaReturnStat::cast(node.clone()).is_some() {
        return true;
    }
    node.children().any(|child| may_return(&child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast_indexer::FileIndex;
    #[test]
    fn uncertain_returns_stop_load_proof_but_nested_function_returns_do_not() {
        for (source, expected) in [
            ("if config then return end include('child.lua')", 0),
            (
                "for i=1,3 do if config then return end end include('child.lua')",
                0,
            ),
            (
                "local fn=function() return end include('child.lua')",
                SERVER | CLIENT,
            ),
            (
                "if config then function api.Nested() return end end include('child.lua')",
                SERVER | CLIENT,
            ),
        ] {
            let sources = HashMap::from([
                ("lua/includes/init.lua".into(), source.into()),
                (
                    "lua/includes/child.lua".into(),
                    "function api.Child() end".into(),
                ),
            ]);
            let graph = LoadGraph::build(&sources);
            let def =
                FileIndex::parse_file("lua/includes/child.lua", &sources["lua/includes/child.lua"])
                    .definitions
                    .remove(0);
            assert_eq!(
                graph.symbol_realms("lua/includes/child.lua", &def),
                expected,
                "{source}"
            );
        }
    }
    #[test]
    fn literal_include_require_and_realm_guards_trace_real_entrypoints() {
        let sources = HashMap::from([
            ("lua/includes/init.lua".into(), "if SERVER then include('server.lua') else include('client.lua') end require('demo') AddCSLuaFile('sent.lua')".into()),
            ("lua/includes/server.lua".into(), "function api.Server() end".into()),
            ("lua/includes/client.lua".into(), "function api.Client() end".into()),
            ("lua/includes/modules/demo.lua".into(), "function api.Shared() end".into()),
            ("lua/includes/sent.lua".into(), "function api.NotLoaded() end".into()),
        ]);
        let graph = LoadGraph::build(&sources);
        for (file, mask) in [
            ("server.lua", SERVER),
            ("client.lua", CLIENT),
            ("modules/demo.lua", SERVER | CLIENT),
            ("sent.lua", 0),
        ] {
            let file = format!("lua/includes/{}", file);
            let def = FileIndex::parse_file(&file, &sources[&file])
                .definitions
                .remove(0);
            assert_eq!(graph.symbol_realms(&file, &def), mask);
        }
    }
    #[test]
    fn client_includes_menu_but_client_dll_does_not_and_registration_guards_apply() {
        let source = "if CLIENT then function api.Both() end end if CLIENT_DLL then function api.OnlyClient() end end local P={} function P:Method() end if MENU_DLL then vgui.Register('DMenu', P) end";
        let sources = HashMap::from([("lua/vgui/test.lua".into(), source.into())]);
        let graph = LoadGraph::build(&sources);
        let index = FileIndex::parse_file("lua/vgui/test.lua", source);
        for (name, mask) in [
            ("Both", CLIENT | MENU),
            ("OnlyClient", CLIENT),
            ("Method", MENU),
        ] {
            let def = index
                .definitions
                .iter()
                .find(|def| def.base_name == name)
                .unwrap();
            assert_eq!(graph.symbol_realms("lua/vgui/test.lua", def), mask);
        }
    }
    #[test]
    fn early_returns_shadowed_globals_cycles_and_dynamic_paths_do_not_invent_proof() {
        let source = "if SERVER then return end include('child.lua') local CLIENT = unknown if CLIENT then include('unproven.lua') end include(dynamic)";
        let sources = HashMap::from([
            ("lua/includes/init.lua".into(), source.into()),
            (
                "lua/includes/child.lua".into(),
                "include('init.lua') function api.Child() end".into(),
            ),
            (
                "lua/includes/unproven.lua".into(),
                "function api.Unknown() end".into(),
            ),
        ]);
        let graph = LoadGraph::build(&sources);
        let def =
            FileIndex::parse_file("lua/includes/child.lua", &sources["lua/includes/child.lua"])
                .definitions
                .remove(0);
        assert_eq!(graph.symbol_realms("lua/includes/child.lua", &def), CLIENT);
        assert!(
            !graph
                .active
                .keys()
                .any(|(file, _)| file.ends_with("unproven.lua"))
        );
        assert!(
            graph
                .unresolved
                .iter()
                .any(|(_, _, reason)| reason.contains("dynamic"))
        );
    }
}
