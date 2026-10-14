use crate::ast_indexer::{FileIndex, LineIndex, Realm, SymbolDefinition, format_wiki_line_attr};
use glua_parser::{
    LuaAstNode, LuaAstToken, LuaBlock, LuaClosureExpr, LuaExpr, LuaIndexKey, LuaParser, LuaStat,
    LuaSyntaxNode, LuaVarExpr,
};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Unknown,
    Table(usize),
    Function(Site),
    // A reference to a global does not prove that its value is a function.
    GlobalPath(String),
    NativeFunction(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Site {
    start: usize,
    end: usize,
    realm: Realm,
    start_offset: usize,
    end_offset: usize,
    contract: Option<crate::function_contract::FunctionContract>,
}

#[derive(Default)]
struct Table {
    members: HashMap<String, Value>,
    exports: BTreeMap<String, bool>, // name -> class (colon) or library (dot)
    functions: Vec<(String, Site, usize)>,
    export_guards: HashMap<String, usize>,
}

struct Indexer {
    scopes: Vec<HashMap<String, Value>>,
    tables: Vec<Table>,
    namespaces: HashMap<String, usize>,
    environment: usize,
    line_index: LineIndex,
    base_realm: Realm,
    class_bases: HashMap<String, String>,
    control_lines: HashMap<String, (usize, usize)>,
}

pub fn index_file(path: &str, source: &str) -> FileIndex {
    let tree = LuaParser::parse(source, crate::ast_indexer::parser_config());
    let mut indexer = Indexer {
        scopes: vec![HashMap::new()],
        tables: vec![Table::default()],
        namespaces: HashMap::from([("_G".to_string(), 0)]),
        environment: 0,
        line_index: LineIndex::new(source),
        base_realm: FileIndex::detect_base_file_realm(path, source),
        class_bases: HashMap::new(),
        control_lines: HashMap::new(),
    };
    indexer.tables[0].exports.insert("_G".into(), false);
    // Some VGUI loaders provide a per-file PANEL prototype. It becomes a public
    // class only when registration supplies a concrete name; it is not a library.
    let panel_prototype = indexer.new_table();
    indexer.tables[0]
        .members
        .insert("PANEL".into(), Value::Table(panel_prototype));
    // These runtime library tables can be referenced through local aliases even
    // when their engine-provided initialization is absent from the Lua repo.
    for name in [
        "table",
        "string",
        "math",
        "util",
        "file",
        "net",
        "hook",
        "vgui",
        "draw",
        "surface",
        "http",
        "coroutine",
        "bit",
        "debug",
        "cam",
        "render",
        "cvars",
        "concommand",
        "sound",
        "team",
        "timer",
        "weapons",
        "effects",
        "derma",
        "halo",
        "properties",
        "drive",
        "markup",
        "jit",
        "motionsensor",
        "steamworks",
        "video",
        "input",
        "gui",
        "gamemode",
        "cleanup",
        "player_manager",
        "ai_schedule",
        "ai_task",
        "player",
        "ents",
        "sql",
        "saverestore",
        "notification",
        "menubar",
        "killicon",
        "cookie",
        "controlpanel",
        "construct",
        "constraint",
        "chat",
        "broadcast",
    ] {
        let id = indexer.namespace(name);
        indexer.tables[0]
            .members
            .insert(name.into(), Value::Table(id));
    }
    for name in ["vgui.Register", "derma.DefineControl"] {
        let (parent, member) = name.split_once('.').unwrap();
        let id = indexer.namespaces[parent];
        indexer.tables[id]
            .members
            .insert(member.into(), Value::GlobalPath(name.into()));
    }
    for name in ["string.format", "net.WriteBit", "vgui.Create", "table.Copy"] {
        let (parent, member) = name.split_once('.').unwrap();
        let id = indexer.namespaces[parent];
        indexer.tables[id]
            .members
            .insert(member.into(), Value::NativeFunction(name.into()));
    }
    // Known callable builtins, not arbitrary identifier-like right-hand sides.
    for name in [
        "assert",
        "error",
        "ipairs",
        "next",
        "pairs",
        "pcall",
        "print",
        "rawequal",
        "rawget",
        "rawset",
        "select",
        "setmetatable",
        "getmetatable",
        "tonumber",
        "tostring",
        "type",
        "unpack",
        "xpcall",
        "tobool",
        "isbool",
        "isfunction",
        "isnumber",
        "isstring",
        "istable",
        "isvector",
        "isangle",
        "IsValid",
    ] {
        indexer.tables[0]
            .members
            .insert(name.into(), Value::NativeFunction(name.into()));
    }
    if let Some(block) = tree.get_chunk_node().get_block() {
        indexer.block(&block, false);
    }
    let mut definitions = Vec::new();
    let mut seen = HashSet::new();
    for table in &indexer.tables {
        for (namespace, class) in &table.exports {
            for (name, site, definition_offset) in &table.functions {
                let parent = (namespace != "_G").then(|| namespace.clone());
                let full_name = match &parent {
                    Some(parent) => format!("{}{}{}", parent, if *class { ':' } else { '.' }, name),
                    None => name.clone(),
                };
                if !seen.insert((
                    full_name.clone(),
                    site.start_offset,
                    site.end_offset,
                    site.realm,
                )) {
                    continue;
                }
                definitions.push(SymbolDefinition {
                    full_name,
                    parent,
                    base_name: name.clone(),
                    start_line: site.start,
                    end_line: site.end,
                    source_start_offset: site.start_offset,
                    source_end_offset: site.end_offset,
                    wiki_line_attr: format_wiki_line_attr(site.start, site.end),
                    realm: site.realm,
                    contract: site.contract.clone(),
                    definition_offset: *definition_offset,
                    registration_offset: table.export_guards.get(namespace).copied(),
                });
            }
        }
    }
    definitions.sort_by(|a, b| {
        a.start_line
            .cmp(&b.start_line)
            .then(a.full_name.cmp(&b.full_name))
    });
    FileIndex {
        relative_path: path.into(),
        definitions,
        class_bases: indexer.class_bases,
        control_lines: indexer.control_lines,
    }
}

impl Indexer {
    fn site(&self, node: &LuaSyntaxNode) -> Site {
        let range = node.text_range();
        let (start, end) = self
            .line_index
            .get_line_range(range.start().into(), range.end().into());
        Site {
            start,
            end,
            realm: FileIndex::determine_node_realm(node, self.base_realm, |name| {
                self.lookup(name) == Value::GlobalPath(name.into())
            }),
            start_offset: range.start().into(),
            end_offset: range.end().into(),
            contract: crate::function_contract::contract(node),
        }
    }

    fn new_table(&mut self) -> usize {
        let id = self.tables.len();
        self.tables.push(Table::default());
        id
    }

    fn lookup(&self, name: &str) -> Value {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return value.clone();
            }
        }
        if name == "_G" {
            return Value::Table(0);
        }
        self.tables[self.environment]
            .members
            .get(name)
            .cloned()
            .unwrap_or_else(|| {
                Value::GlobalPath(if self.environment == 0 {
                    name.into()
                } else {
                    let namespace = self.tables[self.environment].exports.keys().next().unwrap();
                    format!("{}.{}", namespace, name)
                })
            })
    }

    fn namespace(&mut self, name: &str) -> usize {
        if let Some(id) = self.namespaces.get(name) {
            return *id;
        }
        let id = self.new_table();
        self.tables[id].exports.insert(name.into(), false);
        self.namespaces.insert(name.into(), id);
        id
    }

    fn table_value(&mut self, value: Value) -> Option<usize> {
        match value {
            Value::Table(id) => Some(id),
            Value::GlobalPath(path) => Some(self.namespace(&path)),
            _ => None,
        }
    }

    fn member(&self, value: Value, name: &str) -> Value {
        match value {
            Value::Table(id) => self.tables[id]
                .members
                .get(name)
                .cloned()
                .unwrap_or_else(|| {
                    self.tables[id]
                        .exports
                        .keys()
                        .next()
                        .map_or(Value::Unknown, |parent| {
                            Value::GlobalPath(if parent == "_G" {
                                name.into()
                            } else {
                                format!("{}.{}", parent, name)
                            })
                        })
                }),
            Value::GlobalPath(path) => {
                if let Some(id) = self.namespaces.get(&path) {
                    self.tables[*id]
                        .members
                        .get(name)
                        .cloned()
                        .unwrap_or(Value::Unknown)
                } else {
                    Value::GlobalPath(format!("{}.{}", path, name))
                }
            }
            _ => Value::Unknown,
        }
    }

    fn set_member(&mut self, id: usize, name: &str, value: Value, node: &LuaSyntaxNode) {
        let value = match value {
            Value::NativeFunction(_) => Value::Function(self.site(node)),
            value => value,
        };
        if let Value::Function(site) = &value {
            self.tables[id].functions.push((
                name.into(),
                site.clone(),
                node.text_range().start().into(),
            ));
        }
        if let Value::Table(child) = &value {
            let class_alias = self.tables[*child].exports.values().any(|class| *class);
            let names: Vec<_> = self.tables[id].exports.keys().cloned().collect();
            for parent in names {
                if class_alias {
                    continue;
                }
                let namespace = if parent == "_G" {
                    name.into()
                } else {
                    format!("{}.{}", parent, name)
                };
                self.export_table(
                    *child,
                    &namespace,
                    false,
                    Some(node.text_range().start().into()),
                );
            }
        }
        self.tables[id].members.insert(name.into(), value);
    }

    fn export_table(&mut self, id: usize, name: &str, class: bool, guard: Option<usize>) {
        if self.tables[id].exports.insert(name.into(), class).is_some() {
            return;
        }
        self.namespaces.insert(name.into(), id);
        if let Some(guard) = guard {
            self.tables[id].export_guards.insert(name.into(), guard);
        }
        let children: Vec<_> = self.tables[id]
            .members
            .iter()
            .filter_map(|(key, value)| {
                if let Value::Table(child) = value {
                    Some((key.clone(), *child))
                } else {
                    None
                }
            })
            .collect();
        for (key, child) in children {
            // Avoid cycles such as T.__index = T and references to other public tables.
            if child != id && self.tables[child].exports.is_empty() {
                self.export_table(child, &format!("{}.{}", name, key), false, guard);
            }
        }
    }

    fn assign(&mut self, target: &LuaVarExpr, value: Value, node: &LuaSyntaxNode) {
        match target {
            LuaVarExpr::NameExpr(name) => {
                let name = name.get_text();
                for scope in self.scopes.iter_mut().rev() {
                    if scope.contains_key(&name) {
                        scope.insert(name, value);
                        return;
                    }
                }
                self.set_member(self.environment, &name, value, node);
            }
            LuaVarExpr::IndexExpr(index) => {
                if let (Some(prefix), Some(key)) = (
                    index.get_prefix_expr(),
                    index.get_index_key().and_then(static_key),
                ) {
                    let receiver = self.expr(&prefix);
                    if let Some(id) = self.table_value(receiver) {
                        self.set_member(id, &key, value, node);
                    }
                }
            }
        }
    }

    fn expr(&mut self, expr: &LuaExpr) -> Value {
        match expr {
            LuaExpr::NameExpr(name) => self.lookup(&name.get_text()),
            LuaExpr::IndexExpr(index) => {
                if let (Some(prefix), Some(key)) = (
                    index.get_prefix_expr(),
                    index.get_index_key().and_then(static_key),
                ) {
                    let receiver = self.expr(&prefix);
                    self.member(receiver, &key)
                } else {
                    Value::Unknown
                }
            }
            LuaExpr::ParenExpr(paren) => paren
                .get_expr()
                .map(|expr| self.expr(&expr))
                .unwrap_or(Value::Unknown),
            LuaExpr::ClosureExpr(closure) => {
                let value = Value::Function(self.site(closure.syntax()));
                self.function_body(closure, false);
                value
            }
            LuaExpr::TableExpr(table) => {
                let id = self.new_table();
                for field in table.get_fields() {
                    if let Some(value) = field.get_value_expr() {
                        let value = self.expr(&value);
                        if let Some(key) = field.get_field_key().and_then(static_key) {
                            self.set_member(id, &key, value, field.syntax());
                        }
                    }
                }
                Value::Table(id)
            }
            LuaExpr::CallExpr(call) => self.call(call),
            // Coalescing namespace initializers, e.g. util = util or {}.
            LuaExpr::BinaryExpr(binary)
                if binary
                    .get_op_token()
                    .is_some_and(|op| matches!(op.get_text(), "or" | "||")) =>
            {
                let expressions: Vec<_> = binary
                    .syntax()
                    .children()
                    .filter_map(LuaExpr::cast)
                    .collect();
                let values: Vec<_> = expressions.iter().map(|expr| self.expr(expr)).collect();
                values
                    .iter()
                    .find(|value| matches!(value, Value::Table(_)))
                    .cloned()
                    .unwrap_or(Value::Unknown)
            }
            _ => {
                // Still visit closures used as call arguments and nested expressions.
                for child in expr.syntax().children().filter_map(LuaExpr::cast) {
                    self.expr(&child);
                }
                Value::Unknown
            }
        }
    }

    fn call(&mut self, call: &glua_parser::LuaCallExpr) -> Value {
        let target = call
            .get_prefix_expr()
            .map(|expr| self.expr(&expr))
            .unwrap_or(Value::Unknown);
        let args: Vec<_> = call
            .get_args_list()
            .map(|args| args.get_args().collect())
            .unwrap_or_default();
        let values: Vec<_> = args.iter().map(|expr| self.expr(expr)).collect();
        if let Value::NativeFunction(name) = &target {
            if name == "setmetatable" {
                return values.first().cloned().unwrap_or(Value::Unknown);
            }
            if name == "table.Copy" {
                if let Some(Value::Table(original)) = values.first() {
                    let members = self.tables[*original].members.clone();
                    let functions = self.tables[*original].functions.clone();
                    let id = self.new_table();
                    self.tables[id].members = members;
                    self.tables[id].functions = functions;
                    return Value::Table(id);
                }
            }
        }
        let Value::GlobalPath(target) = target else {
            return Value::Unknown;
        };
        match target.as_str() {
            "RegisterMetaTable" => {
                if let (Some(name), Some(Value::Table(id))) =
                    (args.first().and_then(string_literal), values.get(1))
                {
                    // Registration owns the public metatable name; local spelling is private.
                    self.export_table(
                        *id,
                        &name,
                        true,
                        Some(call.syntax().text_range().start().into()),
                    );
                }
            }
            "FindMetaTable" => {
                if let Some(name) = args.first().and_then(string_literal) {
                    let id = self.namespace(&name);
                    self.tables[id].exports.insert(name, true);
                    return Value::Table(id);
                }
            }
            "module" => {
                if let Some(name) = args.first().and_then(string_literal) {
                    self.environment = self.namespace(&name);
                }
            }
            "vgui.Register" | "derma.DefineControl" => {
                let table_arg = if target == "vgui.Register" { 1 } else { 2 };
                if let (Some(name), Some(Value::Table(id))) =
                    (args.first().and_then(string_literal), values.get(table_arg))
                {
                    let id = *id;
                    self.export_table(
                        id,
                        &name,
                        true,
                        Some(call.syntax().text_range().start().into()),
                    );
                    let base = args
                        .get(table_arg + 1)
                        .and_then(string_literal)
                        .unwrap_or_else(|| "Panel".into());
                    let site = self.site(call.syntax());
                    self.control_lines
                        .insert(name.clone(), (site.start, site.end));
                    self.class_bases.insert(name, base);
                    return Value::Table(id);
                }
            }
            "AccessorFunc" => {
                if let (Some(receiver), Some(name)) = (
                    values.first().cloned(),
                    args.get(2).and_then(string_literal),
                ) {
                    if let Some(id) = self.table_value(receiver) {
                        for prefix in ["Get", "Set"] {
                            self.set_member(
                                id,
                                &format!("{}{}", prefix, name),
                                Value::Function(self.site(call.syntax())),
                                call.syntax(),
                            );
                        }
                    }
                }
            }
            "Derma_Hook" => {
                if let (Some(Value::Table(id)), Some(name)) =
                    (values.first(), args.get(1).and_then(string_literal))
                {
                    self.set_member(
                        *id,
                        &name,
                        Value::Function(self.site(call.syntax())),
                        call.syntax(),
                    );
                }
            }
            _ => {}
        }
        // Runtime objects (vgui.Create, Angle, Entity, etc.) are not API namespaces.
        Value::Unknown
    }

    fn function_body(&mut self, closure: &LuaClosureExpr, method: bool) {
        // Function bodies do not execute at declaration time. Preserve outer bindings.
        let saved_scopes = self.scopes.clone();
        let saved_environment = self.environment;
        let saved_members: Vec<_> = self
            .tables
            .iter()
            .map(|table| table.members.clone())
            .collect();
        self.scopes.push(HashMap::new());
        if method {
            self.scopes
                .last_mut()
                .unwrap()
                .insert("self".into(), Value::Unknown);
        }
        if let Some(params) = closure.get_params_list() {
            for param in params.get_params() {
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(param.get_text(), Value::Unknown);
            }
        }
        if let Some(block) = closure.get_block() {
            self.block(&block, false);
        }
        self.scopes = saved_scopes;
        self.environment = saved_environment;
        for (table, members) in self.tables.iter_mut().zip(saved_members) {
            table.members = members;
        }
    }

    fn block(&mut self, block: &LuaBlock, scoped: bool) {
        if scoped {
            self.scopes.push(HashMap::new());
        }
        for stat in block.get_stats() {
            match stat {
                LuaStat::LocalStat(local) => {
                    // All RHS values are evaluated before any new local becomes visible.
                    let values: Vec<_> = local
                        .get_value_exprs()
                        .map(|expr| self.expr(&expr))
                        .collect();
                    for (idx, name) in local.get_local_name_list().enumerate() {
                        let value = values.get(idx).cloned().unwrap_or(Value::Unknown);
                        // Copying an unresolved global value does not establish a public
                        // library. Known table aliases and registration-function aliases
                        // retain their identity; arbitrary runtime objects do not.
                        let value = match value {
                            Value::GlobalPath(path)
                                if !matches!(
                                    path.as_str(),
                                    "vgui.Register"
                                        | "derma.DefineControl"
                                        | "FindMetaTable"
                                        | "AccessorFunc"
                                ) =>
                            {
                                Value::Unknown
                            }
                            value => value,
                        };
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(name.get_text(), value);
                    }
                }
                LuaStat::LocalFuncStat(func) => {
                    if let (Some(name), Some(closure)) = (func.get_local_name(), func.get_closure())
                    {
                        let value = Value::Function(self.site(func.syntax()));
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(name.get_text(), value);
                        self.function_body(&closure, false);
                    }
                }
                LuaStat::FuncStat(func) => {
                    if let (Some(name), Some(closure)) = (func.get_func_name(), func.get_closure())
                    {
                        self.assign(
                            &name,
                            Value::Function(self.site(func.syntax())),
                            func.syntax(),
                        );
                        self.function_body(&closure, name.get_text().contains(':'));
                    }
                }
                LuaStat::AssignStat(assign) => {
                    let (targets, expressions) = assign.get_var_and_expr_list();
                    let values: Vec<_> = expressions.iter().map(|expr| self.expr(expr)).collect();
                    let compound = assign
                        .get_assign_op()
                        .is_some_and(|op| op.get_text() != "=");
                    for (idx, target) in targets.iter().enumerate() {
                        self.assign(
                            target,
                            if compound {
                                Value::Unknown
                            } else {
                                values.get(idx).cloned().unwrap_or(Value::Unknown)
                            },
                            assign.syntax(),
                        );
                    }
                }
                LuaStat::CallExprStat(stat) => {
                    if let Some(call) = stat.get_call_expr() {
                        self.call(&call);
                    }
                }
                LuaStat::IfStat(stat) => {
                    let saved_scopes = self.scopes.clone();
                    let saved_environment = self.environment;
                    let saved_members: Vec<_> = self
                        .tables
                        .iter()
                        .map(|table| table.members.clone())
                        .collect();
                    let mut branches = Vec::new();
                    if let Some(condition) = stat.get_condition_expr() {
                        self.expr(&condition);
                    }
                    if let Some(block) = stat.get_block() {
                        self.block(&block, true);
                    }
                    branches.push(self.scopes.clone());
                    let clauses: Vec<_> = stat.get_all_clause().collect();
                    for clause in clauses {
                        self.scopes = saved_scopes.clone();
                        self.environment = saved_environment;
                        for (table, members) in self.tables.iter_mut().zip(&saved_members) {
                            table.members = members.clone();
                        }
                        self.children(clause.syntax());
                        branches.push(self.scopes.clone());
                    }
                    if stat.get_else_clause().is_none() {
                        branches.push(saved_scopes.clone());
                    }
                    self.scopes = saved_scopes;
                    self.environment = saved_environment;
                    for (idx, scope) in self.scopes.iter_mut().enumerate() {
                        for (name, value) in scope {
                            let first = branches[0][idx]
                                .get(name)
                                .cloned()
                                .unwrap_or(Value::Unknown);
                            *value = if branches
                                .iter()
                                .all(|branch| branch[idx].get(name) == Some(&first))
                            {
                                first
                            } else {
                                Value::Unknown
                            };
                        }
                    }
                    for (table, members) in self.tables.iter_mut().zip(saved_members) {
                        table.members = members;
                    }
                }
                LuaStat::ForStat(stat) => {
                    let before = self.scopes.clone();
                    for expr in stat.get_iter_expr() {
                        self.expr(&expr);
                    }
                    self.scopes.push(HashMap::new());
                    if let Some(name) = stat.get_var_name() {
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(name.get_text().to_string(), Value::Unknown);
                    }
                    if let Some(block) = stat.get_block() {
                        self.block(&block, false);
                    }
                    self.scopes.pop();
                    self.invalidate_loop_assignments(before);
                }
                LuaStat::ForRangeStat(stat) => {
                    let before = self.scopes.clone();
                    for expr in stat.get_expr_list() {
                        self.expr(&expr);
                    }
                    self.scopes.push(HashMap::new());
                    for name in stat.get_var_name_list() {
                        self.scopes
                            .last_mut()
                            .unwrap()
                            .insert(name.get_text().to_string(), Value::Unknown);
                    }
                    if let Some(block) = stat.get_block() {
                        self.block(&block, false);
                    }
                    self.scopes.pop();
                    self.invalidate_loop_assignments(before);
                }
                LuaStat::WhileStat(stat) => {
                    let before = self.scopes.clone();
                    if let Some(condition) = stat.get_condition_expr() {
                        self.expr(&condition);
                    }
                    if let Some(block) = stat.get_block() {
                        self.block(&block, true);
                    }
                    self.invalidate_loop_assignments(before);
                }
                LuaStat::RepeatStat(stat) => {
                    let before = self.scopes.clone();
                    self.scopes.push(HashMap::new());
                    if let Some(block) = stat.get_block() {
                        self.block(&block, false);
                    }
                    // Repeat-block locals remain visible in the until condition.
                    if let Some(condition) = stat.get_condition_expr() {
                        self.expr(&condition);
                    }
                    self.scopes.pop();
                    self.invalidate_loop_assignments(before);
                }
                _ => self.children(stat.syntax()),
            }
        }
        if scoped {
            self.scopes.pop();
        }
    }

    fn invalidate_loop_assignments(&mut self, mut before: Vec<HashMap<String, Value>>) {
        for (old, current) in before.iter_mut().zip(&self.scopes) {
            for (name, value) in old {
                if current.get(name) != Some(value) {
                    *value = Value::Unknown;
                }
            }
        }
        self.scopes = before;
    }

    fn children(&mut self, node: &LuaSyntaxNode) {
        for child in node.children() {
            if let Some(block) = LuaBlock::cast(child.clone()) {
                self.block(&block, true);
            } else if let Some(expr) = LuaExpr::cast(child.clone()) {
                self.expr(&expr);
            } else {
                self.children(&child);
            }
        }
    }
}

fn string_literal(expr: &LuaExpr) -> Option<String> {
    let LuaExpr::LiteralExpr(literal) = expr else {
        return None;
    };
    let glua_parser::LuaLiteralToken::String(token) = literal.get_literal()? else {
        return None;
    };
    Some(token.get_value())
}

fn static_key(key: LuaIndexKey) -> Option<String> {
    let name = match key {
        LuaIndexKey::Name(token) => token.get_text().to_string(),
        LuaIndexKey::String(token) => token.get_value(),
        _ => return None,
    };
    let mut chars = name.chars();
    (chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_'))
    .then_some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str) -> Vec<String> {
        let mut names: Vec<_> = index_file("lua/test.lua", source)
            .definitions
            .into_iter()
            .map(|def| def.full_name)
            .collect();
        names.sort();
        names
    }

    #[test]
    fn runtime_object_callbacks_and_angle_fields_are_not_apis() {
        assert_eq!(
            names(
                r#"
            function Public(ply, driving)
                local Button = vgui.Create("DImageButton")
                Button.OnDrop = function(s, target) return vgui.Create("ContentHeader", target) end
                local DirToEnt = driving:GetPos() - ply:GetPos()
                local AimAng = DirToEnt:Angle()
                if AimAng.p > 180 then AimAng.p = AimAng.p - 360 end
                AimAng.p = 0
                AimAng.r = 0
            end
        "#
            ),
            vec!["Public"]
        );
    }

    #[test]
    fn unexported_locals_parameters_and_implicit_self_are_private() {
        assert_eq!(
            names(
                r#"
            local Private = {}
            function Private:Helper() end
            local function hidden() end
            local hidden_closure = function() end
            function api.Public(api, object)
                api.Hidden = function() end
                object.Callback = function() end
                function object:OtherCallback() end
            end
            function api:Method()
                self.Callback = function() end
            end
        "#
            ),
            vec!["api.Method", "api.Public"]
        );
    }

    #[test]
    fn locals_apply_after_declaration_and_do_not_escape_blocks() {
        assert_eq!(
            names(
                r#"
            function api.Before() end
            do
                local api = {}
                function api.Hidden() end
            end
            function api.After() end
            local api = {}
            function api.HiddenToo() end
        "#
            ),
            vec!["api.After", "api.Before"]
        );
    }

    #[test]
    fn aliases_keep_the_identity_of_exported_libraries() {
        assert_eq!(
            names(
                r#"
            local lib = math
            function lib.Public() end
            do
                local lib = {}
                lib.Hidden = function() end
            end
            lib = {}
            function lib.HiddenToo() end
        "#
            ),
            vec!["math.Public"]
        );
    }

    #[test]
    fn local_class_aliases_cannot_leak_across_shadowed_bindings() {
        assert_eq!(
            names(
                r#"
            local meta = FindMetaTable("Player")
            function meta:Public() end
            do
                local meta = {}
                function meta:Private() end
            end
            function Setup(meta)
                function meta:Callback() end
            end
            function meta:OtherPublic() end
        "#
            ),
            vec!["Player:OtherPublic", "Player:Public", "Setup"]
        );
    }

    #[test]
    fn registration_publishes_only_the_supplied_table() {
        assert_eq!(
            names(
                r#"
            local PANEL = {}
            function PANEL:First() end
            do
                local PANEL = {}
                function PANEL:Hidden() end
            end
            derma.DefineControl("DNewControl", "Description", PANEL, "DPanel")
            PANEL = {}
            function PANEL:Second() end
            vgui.Register("DOtherControl", PANEL, "Panel")
        "#
            ),
            vec!["DNewControl:First", "DOtherControl:Second"]
        );
    }

    #[test]
    fn registered_metatable_methods_match_public_parent() {
        let registered = FileIndex::parse_file(
            "lua/test.lua",
            r#"
            local COLOR = {}
            RegisterMetaTable("Color", COLOR)
            function COLOR:Copy() return self end
        "#,
        );
        let retrieved = FileIndex::parse_file(
            "lua/test.lua",
            r#"
            local COLOR = FindMetaTable("Color")
            function COLOR:Copy() return self end
        "#,
        );
        println!(
            "RegisterMetaTable exports: {:?}",
            registered
                .definitions
                .iter()
                .map(|d| &d.full_name)
                .collect::<Vec<_>>()
        );
        println!(
            "FindMetaTable exports: {:?}",
            retrieved
                .definitions
                .iter()
                .map(|d| &d.full_name)
                .collect::<Vec<_>>()
        );
        assert!(retrieved.find_symbol("Copy", Some("Color")).is_some());
        assert!(registered.find_symbol("Copy", Some("Color")).is_some());
        let shadowed = FileIndex::parse_file(
            "lua/test.lua",
            r#"
            local RegisterMetaTable = function() end
            local COLOR = {}
            RegisterMetaTable("Color", COLOR)
            function COLOR:Copy() return self end
        "#,
        );
        assert!(shadowed.find_symbol("Copy", Some("Color")).is_none());
    }

    #[test]
    fn shadowed_registration_functions_do_not_publish_private_tables() {
        assert!(
            names(
                r#"
            local vgui = { Register = function() end }
            local PANEL = {}
            function PANEL:Hidden() end
            vgui.Register("DNotRegistered", PANEL)
        "#
            )
            .is_empty()
        );
    }

    #[test]
    fn exporting_private_functions_and_tables_uses_the_public_name() {
        assert_eq!(
            names(
                r#"
            local function helper() end
            local T = { Nested = { Run = function() end } }
            function T.Public() end
            T.Export = helper
            NewLibrary = T
        "#
            ),
            vec![
                "NewLibrary.Export",
                "NewLibrary.Nested.Run",
                "NewLibrary.Public"
            ]
        );
    }

    #[test]
    fn values_and_unknown_aliases_are_not_callable_definitions() {
        assert_eq!(
            names(
                r#"
            math.tau = 6.28318
            math.otherPi = math.pi
            api.Flag = true
            api.Nothing = nil
            api.Value = UNKNOWN_VALUE
            api.Native = tostring
            function api.Real() end
            api.Alias = api.Real
        "#
            ),
            vec!["api.Alias", "api.Native", "api.Real"]
        );
    }

    #[test]
    fn loop_variables_shadow_public_libraries() {
        assert_eq!(
            names(
                r#"
            for math, value in pairs(items) do
                function math.Hidden() end
                value.Callback = function() end
            end
            for math = 1, 10 do function math.HiddenToo() end end
            function math.Public() end
        "#
            ),
            vec!["math.Public"]
        );
    }

    #[test]
    fn ambiguous_branch_aliases_are_not_promoted_to_apis() {
        assert_eq!(
            names(
                r#"
            local lib = {}
            if FLAG then lib = math end
            function lib.NotProvenPublic() end
            if FLAG then
                local panel = vgui.Create("DPanel")
                panel.Paint = function() end
            else
                local panel = {}
                panel.Private = function() end
            end
        "#
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn function_bodies_do_not_reassign_outer_bindings_at_declaration_time() {
        assert_eq!(
            names(
                r#"
            local lib = math
            function Setup() lib = {} end
            function lib.Public() end
        "#
            ),
            vec!["Setup", "math.Public"]
        );
    }

    #[test]
    fn modules_and_explicit_global_exports_use_distinct_environments() {
        assert_eq!(
            names(
                r#"
            module("newlib", package.seeall)
            function Public() end
            local function Hidden() end
            _G.ExplicitGlobal = function() end
        "#
            ),
            vec!["ExplicitGlobal", "newlib.Public"]
        );
    }

    #[test]
    fn static_string_keys_work_but_dynamic_indices_are_not_page_names() {
        assert_eq!(
            names(
                r#"
            api["Public"] = function() end
            api[dynamic] = function() end
            api["not a page"] = function() end
            local p = {}
            p["Private"] = function() end
        "#
            ),
            vec!["api.Public"]
        );
    }

    #[test]
    fn global_metatable_aliases_do_not_invent_library_names() {
        assert_eq!(
            names(
                r#"
            Meta = FindMetaTable("Player")
            function Meta:Public() end
        "#
            ),
            vec!["Player:Public"]
        );
    }

    #[test]
    fn alias_initializer_uses_the_previous_binding() {
        assert_eq!(
            names(
                r#"
            local math = math
            function math.Public() end
        "#
            ),
            vec!["math.Public"]
        );
    }

    #[test]
    fn function_range_includes_nested_blocks_and_excludes_the_next_function() {
        let index = index_file(
            "lua/test.lua",
            "function api.One()\n if FLAG then\n  return\n end\nend\n\nfunction api.Two() end\n",
        );
        let first = index
            .definitions
            .iter()
            .find(|def| def.base_name == "One")
            .unwrap();
        assert_eq!((first.start_line, first.end_line), (1, 5));
    }

    #[test]
    fn copied_control_tables_retain_their_methods_without_aliasing_later_changes() {
        assert_eq!(
            names(
                r#"
            local PANEL = {}
            function PANEL:Original() end
            derma.DefineControl("DCopied", "Description", table.Copy(PANEL), "Panel")
            function PANEL:Later() end
            vgui.Register("DOriginal", PANEL, "Panel")
        "#
            ),
            vec!["DCopied:Original", "DOriginal:Later", "DOriginal:Original"]
        );
    }

    #[test]
    fn generated_hooks_and_native_callable_aliases_are_preserved() {
        assert_eq!(
            names(
                r#"
            local PANEL = {}
            Derma_Hook(PANEL, "Paint", "Paint", "Tree")
            vgui.Register("DTest", PANEL, "Panel")
            Format = string.format
            net.WriteBool = net.WriteBit
        "#
            ),
            vec!["DTest:Paint", "Format", "net.WriteBool"]
        );
    }

    #[test]
    fn unnamed_loader_prototypes_are_not_public_panel_apis() {
        assert!(
            names(
                r#"
            PANEL.Base = "DPanel"
            AccessorFunc(PANEL, "value", "Progress")
            function PANEL:StartDownloading() end
        "#
            )
            .is_empty()
        );
    }

    #[test]
    fn registration_return_values_preserve_compatibility_control_identity() {
        assert_eq!(
            names(
                r#"
            local PANEL = {}
            function PANEL:Known() end
            local PANEL = derma.DefineControl("DOriginal", "Original", PANEL, "Panel")
            vgui.Register("DCompatibility", PANEL, "Panel")
        "#
            ),
            vec!["DCompatibility:Known", "DOriginal:Known"]
        );
    }

    #[test]
    fn loop_assignments_do_not_turn_private_objects_into_public_aliases() {
        assert!(
            names(
                r#"
            local t = {}
            while FLAG do t = math end
            function t.NotProvenPublic() end
            local u = {}
            for i = 1, count do u = math end
            function u.NotProvenPublic() end
        "#
            )
            .is_empty()
        );
    }

    #[test]
    fn repeat_locals_remain_visible_in_the_until_condition() {
        assert!(
            names(
                r#"
            repeat
                local math = {}
            until (function()
                function math.Hidden() end
                return true
            end)()
        "#
            )
            .is_empty()
        );
    }
}
