use glua_parser::{
    LuaAstNode, LuaAstToken, LuaElseClauseStat, LuaElseIfClauseStat, LuaFuncStat, LuaIfStat,
    LuaParser, LuaSyntaxNode, ParserConfig,
};
use std::collections::HashMap;
use std::fmt;

pub(crate) fn parser_config() -> ParserConfig<'static> {
    use glua_parser::LuaNonStdSymbol::*;
    let mut symbols = glua_parser::LuaNonStdSymbolSet::new();
    symbols.extends(vec![
        DoubleSlash,
        SlashStar,
        PlusAssign,
        MinusAssign,
        StarAssign,
        SlashAssign,
        PercentAssign,
        CaretAssign,
        DoublePipe,
        DoubleAmp,
        Exclamation,
        NotEqual,
        Continue,
    ]);
    ParserConfig::new(
        glua_parser::LuaLanguageLevel::LuaJIT,
        None,
        HashMap::new(),
        symbols,
        true,
    )
}

/// Execution realm for Garry's Mod Lua code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Realm {
    Unknown,
    SharedAndMenu,
    Shared,
    Server,
    Client,
    Menu,
    ClientAndMenu,
}

impl Realm {
    pub fn as_wiki_str(&self) -> &'static str {
        match self {
            Realm::Unknown => "Unknown",
            Realm::SharedAndMenu => "Shared and Menu",
            Realm::Shared => "Shared",
            Realm::Server => "Server",
            Realm::Client => "Client",
            Realm::Menu => "Menu",
            Realm::ClientAndMenu => "Client and Menu",
        }
    }

    pub fn parse_wiki_str(s: &str) -> Option<Self> {
        let clean = s.trim();
        if clean.eq_ignore_ascii_case("Shared") {
            Some(Realm::Shared)
        } else if clean.eq_ignore_ascii_case("Server") {
            Some(Realm::Server)
        } else if clean.eq_ignore_ascii_case("Client") {
            Some(Realm::Client)
        } else if clean.eq_ignore_ascii_case("Menu") {
            Some(Realm::Menu)
        } else if clean.eq_ignore_ascii_case("Client and Menu")
            || clean.eq_ignore_ascii_case("ClientAndMenu")
        {
            Some(Realm::ClientAndMenu)
        } else if clean.eq_ignore_ascii_case("Shared and Menu") {
            Some(Realm::SharedAndMenu)
        } else {
            None
        }
    }

    /// Checks if a source code realm is compatible with a wiki declared realm.
    pub fn is_compatible_with(&self, wiki_realm: Realm) -> bool {
        // Source placement provides partial availability evidence. Overlapping realm
        // declarations need load-graph evidence before claiming a mismatch.
        fn mask(realm: Realm) -> u8 {
            match realm {
                Realm::Unknown => 7,
                Realm::Server => 1,
                Realm::Client => 2,
                Realm::Menu => 4,
                Realm::Shared => 3,
                Realm::ClientAndMenu => 6,
                Realm::SharedAndMenu => 7,
            }
        }
        mask(*self) & mask(wiki_realm) != 0
    }
}

impl fmt::Display for Realm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_wiki_str())
    }
}

/// Line indexer for converting 0-indexed byte offsets to 1-indexed line numbers.
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self { line_starts }
    }

    /// Converts a 0-indexed byte offset to a 1-indexed line number.
    pub fn offset_to_line(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(line) => line + 1,
            Err(line) => line,
        }
    }

    /// Converts a byte offset span (start, end) to a (start_line, end_line) range (1-indexed).
    pub fn get_line_range(&self, start_offset: usize, end_offset: usize) -> (usize, usize) {
        let start_line = self.offset_to_line(start_offset);
        let end_line = if end_offset > start_offset {
            self.offset_to_line(end_offset - 1)
        } else {
            start_line
        };
        (start_line, end_line)
    }
}

/// A parsed Lua symbol definition with its precise line range in the repository file.
#[derive(Debug, Clone)]
pub struct SymbolDefinition {
    pub alias_target: Option<String>,
    /// Ownership evidence for existing documentation, not a missing-page candidate.
    pub source_only: bool,
    pub full_name: String,
    pub parent: Option<String>,
    pub base_name: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source_start_offset: usize,
    pub source_end_offset: usize,
    pub wiki_line_attr: String,
    pub realm: Realm,
    pub contract: Option<crate::function_contract::FunctionContract>,
    pub definition_offset: usize,
    pub registration_offset: Option<usize>,
}

/// Parsed definitions for a single file.
#[derive(Debug, Clone)]
pub struct FileIndex {
    pub relative_path: String,
    pub definitions: Vec<SymbolDefinition>,
    pub class_bases: HashMap<String, String>,
    /// Named VGUI registrations, including controls with no locally defined methods.
    pub control_lines: HashMap<String, (usize, usize)>,
}

impl FileIndex {
    /// Determines the base realm of a file based on its path and early returns.
    pub(crate) fn detect_base_file_realm(relative_path: &str, source: &str) -> Realm {
        let norm_path = relative_path.replace('\\', "/").to_lowercase();

        // 1. Path-based realm inference
        if norm_path.contains("/vgui/") || norm_path.contains("/derma/") {
            return Realm::ClientAndMenu;
        }
        if norm_path.contains("/client/") {
            return Realm::Client;
        }
        if norm_path.contains("/menu/") {
            return Realm::Menu;
        }
        if norm_path.contains("/server/") {
            return Realm::Server;
        }

        // Only unconditional top-level return guards provide evidence.
        let tree = LuaParser::parse(source, parser_config());
        // A locally rebound realm flag is not an engine constant. Conservatively
        // omit file-guard inference if the file declares one in lexical scope.
        if tree
            .get_chunk_node()
            .syntax()
            .descendants()
            .filter_map(glua_parser::LuaLocalStat::cast)
            .flat_map(|stat| stat.get_local_name_list().collect::<Vec<_>>())
            .filter_map(|name| name.get_name_token())
            .any(|name| {
                matches!(
                    name.get_text(),
                    "SERVER" | "CLIENT" | "CLIENT_DLL" | "MENU_DLL" | "GAME_DLL"
                )
            })
        {
            return Realm::Unknown;
        }
        for child in tree
            .get_chunk_node()
            .get_block()
            .unwrap()
            .syntax()
            .children()
        {
            if child.descendants().any(|n| LuaFuncStat::cast(n).is_some()) {
                break;
            }
            if let Some(guard) = LuaIfStat::cast(child) {
                if guard.get_else_clause().is_none()
                    && guard.get_else_if_clause_list().next().is_none()
                {
                    if let (Some(condition), Some(block)) =
                        (guard.get_condition_expr(), guard.get_block())
                    {
                        if block.get_text().trim() == "return" {
                            if let Some(realm) = condition_realm(&condition.get_text(), false) {
                                return realm;
                            }
                        }
                    }
                }
            }
        }
        Realm::Unknown
    }

    /// Read the actual branch condition, never conditions inside a function body.
    pub(crate) fn determine_node_realm(
        node: &LuaSyntaxNode,
        file_base_realm: Realm,
        flag_visible: impl Fn(&str) -> bool,
    ) -> Realm {
        let mut curr = node.parent();
        while let Some(parent) = curr {
            let condition = if let Some(clause) = LuaElseIfClauseStat::cast(parent.clone()) {
                clause.get_condition_expr().map(|c| (c.get_text(), true))
            } else if LuaElseClauseStat::cast(parent.clone()).is_some() {
                parent.parent().and_then(LuaIfStat::cast).and_then(|stat| {
                    if stat.get_else_if_clause_list().next().is_some() {
                        return None;
                    }
                    stat.get_condition_expr().map(|c| (c.get_text(), false))
                })
            } else if let Some(stat) = LuaIfStat::cast(parent.clone()) {
                // Only the first branch belongs to this condition.
                stat.get_block()
                    .filter(|block| {
                        block
                            .syntax()
                            .text_range()
                            .contains_range(node.text_range())
                    })
                    .and_then(|_| stat.get_condition_expr())
                    .map(|c| (c.get_text(), true))
            } else {
                None
            };
            if let Some((condition, positive)) = condition {
                let clean: String = condition
                    .chars()
                    .filter(|c| !c.is_whitespace() && *c != '(' && *c != ')')
                    .collect();
                let flag = clean.trim_start_matches('!').trim_start_matches("not");
                if let Some(realm) =
                    condition_realm(&condition, positive).filter(|_| flag_visible(flag))
                {
                    return realm;
                }
            }
            curr = parent.parent();
        }
        file_base_realm
    }

    /// Parses a Lua source string and extracts function and symbol definitions.
    pub fn parse_file(relative_path: &str, source: &str) -> Self {
        crate::symbol_indexer::index_file(relative_path, source)
    }

    pub fn parse_file_checked(relative_path: &str, source: &str) -> Result<Self, String> {
        let tree = LuaParser::parse(source, parser_config());
        if tree.has_syntax_errors() {
            return Err(format!(
                "Incomplete source analysis for {relative_path}: {:?}",
                tree.get_errors()
            ));
        }
        Ok(Self::parse_file(relative_path, source))
    }

    /// Finds a symbol definition matching the query and optional preferred realm.
    #[cfg(test)]
    pub fn find_symbol(&self, func_name: &str, parent: Option<&str>) -> Option<&SymbolDefinition> {
        self.find_symbol_with_realm(func_name, parent, None)
    }

    /// Finds a symbol definition matching the query and optional realm filter.
    pub fn find_symbol_with_realm(
        &self,
        func_name: &str,
        parent: Option<&str>,
        preferred_realm: Option<Realm>,
    ) -> Option<&SymbolDefinition> {
        let (query_parent, query_name) = normalize_symbol_query(func_name, parent);
        let matches: Vec<_> = self
            .definitions
            .iter()
            .filter(|def| {
                def.base_name == query_name
                    && (same_symbol_parent(def.parent.as_deref(), query_parent.as_deref())
                        || contextual_script_parent(
                            &self.relative_path,
                            def.parent.as_deref(),
                            query_parent.as_deref(),
                        ))
            })
            .collect();
        // A prototype alias must identify one implementation in the tagged file.
        if matches.len() > 1
            && matches.iter().any(|def| {
                def.source_only
                    || !same_symbol_parent(def.parent.as_deref(), query_parent.as_deref())
            })
        {
            return None;
        }
        if let Some(realm) = preferred_realm {
            if let Some(def) = matches.iter().find(|def| def.realm == realm).or_else(|| {
                matches
                    .iter()
                    .find(|def| def.realm.is_compatible_with(realm))
            }) {
                return Some(def);
            }
        }
        matches.first().copied()
    }
}

pub fn normalize_symbol_query(name: &str, parent: Option<&str>) -> (Option<String>, String) {
    let name = name.trim();
    let address = match parent.map(str::trim).filter(|parent| !parent.is_empty()) {
        Some(parent)
            if !name.starts_with(&format!("{}.", parent))
                && !name.starts_with(&format!("{}:", parent)) =>
        {
            format!("{}.{}", parent, name)
        }
        _ => name.to_string(),
    };
    let (parent, name) = crate::diff_engine::parse_address_parts(&address);
    (
        parent.filter(|parent| parent != "Global" && parent != "_G"),
        name,
    )
}

// Script prototypes are aliases only within the explicitly tagged entity/weapon file.
// Revisit if wiki pages need inherited implementations from multiple script owners;
// that requires an explicit inheritance model, never a repository-wide ENT alias.
pub fn contextual_script_parent(path: &str, source: Option<&str>, wiki: Option<&str>) -> bool {
    let path = path.replace('\\', "/");
    (source == Some("ENT") && wiki == Some("ENTITY") && path.contains("/entities/"))
        || (source == Some("SWEP") && wiki == Some("WEAPON") && path.contains("/weapons/"))
        || (source == Some("ENT")
            && matches!(wiki, Some("NEXTBOT" | "NextBot"))
            && path == "gamemodes/base/entities/entities/base_nextbot/sv_nextbot.lua")
}

pub fn same_symbol_parent(left: Option<&str>, right: Option<&str>) -> bool {
    left == right
        || matches!(
            (left, right),
            (Some("Panel"), Some("PANEL")) | (Some("PANEL"), Some("Panel"))
        )
}

/// Formats start and end line into standard Facepunch wiki attribute `start-Lend` or `start`.
pub fn format_wiki_line_attr(start: usize, end: usize) -> String {
    if start == end {
        format!("{}", start)
    } else {
        format!("{}-L{}", start, end)
    }
}

// CLIENT is true in both Client and Menu; CLIENT_DLL is Client only.
// Unsupported complements and complex expressions remain inconclusive.
fn condition_realm(condition: &str, positive: bool) -> Option<Realm> {
    let clean: String = condition
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '(' && *c != ')')
        .collect();
    match (clean.as_str(), positive) {
        ("SERVER" | "GAME_DLL", true)
        | ("!CLIENT", true)
        | ("notCLIENT", true)
        | ("CLIENT", false)
        | ("!SERVER" | "!GAME_DLL", false)
        | ("notSERVER" | "notGAME_DLL", false) => Some(Realm::Server),
        ("CLIENT", true)
        | ("!SERVER" | "!GAME_DLL", true)
        | ("notSERVER" | "notGAME_DLL", true)
        | ("SERVER" | "GAME_DLL", false)
        | ("!CLIENT", false)
        | ("notCLIENT", false) => Some(Realm::ClientAndMenu),
        ("CLIENT_DLL", true) | ("!CLIENT_DLL" | "notCLIENT_DLL", false) => Some(Realm::Client),
        ("MENU_DLL", true) | ("!MENU_DLL" | "notMENU_DLL", false) => Some(Realm::Menu),
        ("MENU_DLL", false) | ("!MENU_DLL" | "notMENU_DLL", true) => Some(Realm::Shared),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn glua_operators_and_comments_preserve_guarded_metatable_methods() {
        for guard in ["not meta", "(!meta)"] {
            let source = format!(
                "// GLua comment\nlocal meta = FindMetaTable('Entity')\nif {guard} then return end\n/* method */\nfunction meta:Test(x) if x != nil && x > 0 then x += 1 end return true end\nfunction meta:After() end"
            );
            let index = FileIndex::parse_file("lua/test.lua", &source);
            let def = index.find_symbol("Test", Some("Entity")).unwrap();
            assert_eq!(def.start_line, 5);
            assert_eq!(def.end_line, 5);
            assert!(index.find_symbol("After", Some("Entity")).is_some());
            assert_eq!(
                def.contract.as_ref().unwrap().returns,
                Some(vec![vec![Some("boolean".into())]])
            );
        }
    }

    fn realms(source: &str) -> Vec<Realm> {
        FileIndex::parse_file("lua/test.lua", source)
            .definitions
            .iter()
            .map(|d| d.realm)
            .collect()
    }

    #[test]
    fn local_metatable_aliases_expose_real_class_methods() {
        let index = FileIndex::parse_file(
            "lua/test.lua",
            "local meta = FindMetaTable(\"Player\")\nfunction meta:NewMethod() end",
        );
        assert!(
            index
                .definitions
                .iter()
                .any(|d| d.full_name == "Player:NewMethod")
        );
    }

    #[test]
    fn branch_realms_follow_ast_conditions() {
        assert_eq!(
            realms("if SERVER then function A() end else function B() end end"),
            vec![Realm::Server, Realm::ClientAndMenu]
        );
        assert_eq!(
            realms("if CLIENT then function A() end elseif SERVER then function B() end end"),
            vec![Realm::ClientAndMenu, Realm::Server]
        );
        assert_eq!(
            realms("if not SERVER then function A() end end"),
            vec![Realm::ClientAndMenu]
        );
        assert_eq!(
            realms("function A() if SERVER then return end end"),
            vec![Realm::Unknown]
        );
        assert_eq!(
            realms("if SERVER or Something then function A() end end"),
            vec![Realm::Unknown]
        );
    }

    #[test]
    fn file_guards_and_unknown_availability() {
        assert_eq!(
            realms("if SERVER then return end\nfunction A() end"),
            vec![Realm::ClientAndMenu]
        );
        assert_eq!(realms("function A() end"), vec![Realm::Unknown]);
        assert_eq!(
            Realm::parse_wiki_str("Shared and Menu"),
            Some(Realm::SharedAndMenu)
        );
        assert!(!Realm::Shared.is_compatible_with(Realm::Menu));
        assert!(Realm::Client.is_compatible_with(Realm::ClientAndMenu));
        assert!(!Realm::Client.is_compatible_with(Realm::Server));
    }

    #[test]
    fn menu_constants_and_shadowed_flags_do_not_invent_realm_mismatches() {
        assert_eq!(
            realms(
                "if CLIENT then function A() end end if CLIENT_DLL then function B() end end if MENU_DLL then function C() end end"
            ),
            vec![Realm::ClientAndMenu, Realm::Client, Realm::Menu]
        );
        assert_eq!(
            realms("local CLIENT = config if CLIENT then function A() end end"),
            vec![Realm::Unknown]
        );
        assert_eq!(
            realms("local SERVER = config if SERVER then return end function A() end"),
            vec![Realm::Unknown]
        );
        let def = FileIndex::parse_file("lua/vgui/test.lua", "function Test() end")
            .definitions
            .remove(0);
        assert!(def.realm.is_compatible_with(Realm::Menu));
    }

    #[test]
    fn accessors_do_not_invent_is_methods() {
        let index = FileIndex::parse_file(
            "lua/test.lua",
            "local PANEL = {}\nAccessorFunc(PANEL, \"value\", \"Enabled\")\nvgui.Register(\"DTest\", PANEL)",
        );
        assert_eq!(
            index
                .definitions
                .iter()
                .map(|d| d.base_name.as_str())
                .collect::<Vec<_>>(),
            vec!["GetEnabled", "SetEnabled"]
        );
    }
}
