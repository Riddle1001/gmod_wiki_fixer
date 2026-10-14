use crate::ast_indexer::SymbolDefinition;
use crate::function_contract::FunctionContract;
use crate::wiki_markup::{self, Block};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum FindingKind {
    MalformedSource,
    BrokenLink,
    UnverifiedLink,
    IdentityConflict,
    SignatureDrift,
    ReturnDrift,
    LoadRealmConflict,
}

impl FindingKind {
    pub const ALL: [Self; 7] = [
        Self::MalformedSource,
        Self::BrokenLink,
        Self::UnverifiedLink,
        Self::IdentityConflict,
        Self::SignatureDrift,
        Self::ReturnDrift,
        Self::LoadRealmConflict,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::MalformedSource => "Malformed / Duplicate Source Tags",
            Self::BrokenLink => "Broken Wiki Links",
            Self::UnverifiedLink => "Unverified Wiki Links",
            Self::IdentityConflict => "Function Identity Conflicts",
            Self::SignatureDrift => "Signature Review Findings",
            Self::ReturnDrift => "Return Value Review Findings",
            Self::LoadRealmConflict => "Load Path Realm Review Findings",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceEvidence {
    pub file: String,
    pub start: usize,
    pub end: usize,
    pub label: String,
}

impl SourceEvidence {
    pub fn symbol(file: &str, def: &SymbolDefinition) -> Self {
        Self {
            file: file.into(),
            start: def.start_line,
            end: def.end_line,
            label: def.full_name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AuditFinding {
    pub kind: FindingKind,
    pub page: String,
    pub message: String,
    pub sources: Vec<SourceEvidence>,
    pub wiki_line: Option<usize>,
    pub target: Option<String>,
}

impl AuditFinding {
    pub fn new(kind: FindingKind, page: &str, message: String) -> Self {
        Self {
            kind,
            page: page.into(),
            message,
            sources: Vec::new(),
            wiki_line: None,
            target: None,
        }
    }
}

pub fn source_markup_findings(
    page: &str,
    markup: &str,
    file_lines: impl Fn(&str) -> Option<usize>,
) -> Vec<AuditFinding> {
    let files = wiki_markup::blocks(markup, "file");
    let functions = wiki_markup::blocks(markup, "function");
    let mut findings = Vec::new();
    let mut seen = HashSet::new();
    for file in &files {
        let path = wiki_markup::decode(file.inner.trim());
        if !crate::analyzer::source_is_included(&path) {
            continue;
        }
        let line = file.tag.attrs.get("line");
        let mut problems = Vec::new();
        if path.is_empty() {
            problems.push("empty source path".to_string());
        }
        let range = line.and_then(|line| wiki_markup::line_range(line));
        if line.is_none() {
            problems.push("missing line attribute".into());
        } else if range.is_none() {
            problems.push(format!(
                "invalid or inverted line range {:?}",
                line.unwrap()
            ));
        }
        if let (Some((_, end)), Some(total)) = (range, file_lines(&path)) {
            if end > total {
                problems.push(format!(
                    "range ends at {}, but the file has {} lines",
                    end, total
                ));
            }
        }
        let context = functions
            .iter()
            .position(|function| function.tag.start < file.tag.start && file.end <= function.end);
        if !seen.insert((context, path.clone(), line.cloned())) {
            problems.push("duplicate source tag in the same function context".into());
        }
        if !problems.is_empty() {
            let mut finding =
                AuditFinding::new(FindingKind::MalformedSource, page, problems.join("; "));
            finding.wiki_line = Some(wiki_markup::line_at(markup, file.tag.start));
            if let (Some((start, end)), Some(total)) = (range, file_lines(&path)) {
                if total > 0 && start <= total {
                    finding.sources.push(SourceEvidence {
                        file: path,
                        start,
                        end: end.min(total),
                        label: "referenced source".into(),
                    });
                }
            }
            findings.push(finding);
        }
    }
    for tag in wiki_markup::tags(markup)
        .into_iter()
        .filter(|tag| tag.name == "file" && !tag.closing)
    {
        if !files.iter().any(|file| file.tag.start == tag.start) {
            let mut finding = AuditFinding::new(
                FindingKind::MalformedSource,
                page,
                "unclosed source tag".into(),
            );
            finding.wiki_line = Some(wiki_markup::line_at(markup, tag.start));
            findings.push(finding);
        }
    }
    findings
}

pub fn page_references(markup: &str) -> Vec<(String, usize)> {
    let mut seen = HashSet::new();
    wiki_markup::blocks(markup, "page")
        .into_iter()
        .filter_map(|block| {
            if block
                .tag
                .attrs
                .get("realm")
                .is_some_and(|realm| realm != "gmod")
            {
                return None;
            }
            let target = normalize_reference(&wiki_markup::decode(block.inner.trim()))?;
            seen.insert(target.clone())
                .then(|| (target, wiki_markup::line_at(markup, block.tag.start)))
        })
        .collect()
}

pub fn normalize_reference(reference: &str) -> Option<String> {
    let target = reference.split('#').next()?.trim();
    if target.is_empty() || target.contains('<') {
        return None;
    }
    let target = if target.starts_with("http://") || target.starts_with("https://") {
        let url = reqwest::Url::parse(target).ok()?;
        if url.host_str()? != "wiki.facepunch.com" || !url.path().starts_with("/gmod/") {
            return None;
        }
        wiki_markup::decode_uri_path(url.path().trim_start_matches("/gmod/"))
    } else if let Some(target) = target
        .strip_prefix("/gmod/")
        .or_else(|| target.strip_prefix("gmod/"))
    {
        target.to_string()
    } else {
        if target.starts_with('/') || target.starts_with("wiki/") || target.starts_with("rust/") {
            return None;
        }
        target.to_string()
    };
    (!target.is_empty() && !target.starts_with('~')).then_some(target)
}

pub fn documented_arguments(function: &Block) -> (Vec<String>, bool) {
    let args = wiki_markup::blocks(&function.inner, "args");
    let mut fixed = Vec::new();
    let mut variadic = false;
    for arg in args
        .first()
        .map(|args| wiki_markup::outer_blocks(&args.inner, "arg"))
        .unwrap_or_default()
    {
        let name = arg.tag.attrs.get("name").cloned().unwrap_or_default();
        let kind = arg.tag.attrs.get("type").map(String::as_str).unwrap_or("");
        if matches!(kind, "vararg" | "varargs" | "...") || name == "..." {
            variadic = true;
        } else {
            fixed.push(name);
        }
    }
    (fixed, variadic)
}

fn signature_matches(
    contract: &FunctionContract,
    names: &[String],
    variadic: bool,
    method: bool,
) -> bool {
    let mut parameters = contract.parameters.as_slice();
    if method && !contract.implicit_self && parameters.first().is_some_and(|name| name == "self") {
        parameters = &parameters[1..];
    }
    if variadic && !contract.variadic {
        return false;
    }
    if (!contract.variadic && parameters.len() != names.len())
        || (contract.variadic && names.len() < parameters.len())
    {
        return false;
    }
    // Naming differences alone are normal; only flag a demonstrably reordered
    // set of matching identifiers rather than stylistic documentation names.
    let source_set: HashSet<_> = parameters.iter().collect();
    let doc_set: HashSet<_> = names.iter().collect();
    source_set != doc_set || parameters == names
}

pub fn contract_findings(
    page: &str,
    function: &Block,
    candidates: &[(String, SymbolDefinition)],
) -> Vec<AuditFinding> {
    if candidates.is_empty() || candidates.iter().any(|(_, def)| def.contract.is_none()) {
        return Vec::new();
    }
    let method = function
        .tag
        .attrs
        .get("type")
        .is_some_and(|kind| matches!(kind.as_str(), "classfunc" | "hook" | "event"))
        || candidates
            .iter()
            .any(|(_, def)| def.full_name.contains(':'));
    let (names, variadic) = documented_arguments(function);
    let mut findings = Vec::new();
    if !candidates
        .iter()
        .any(|(_, def)| signature_matches(def.contract.as_ref().unwrap(), &names, variadic, method))
    {
        let declarations: Vec<_> = candidates
            .iter()
            .map(|(_, def)| {
                let contract = def.contract.as_ref().unwrap();
                let mut names = contract.parameters.clone();
                if method
                    && !contract.implicit_self
                    && names.first().is_some_and(|name| name == "self")
                {
                    names.remove(0);
                }
                if contract.variadic {
                    names.push("...".into());
                }
                format!("{}({})", def.full_name, names.join(", "))
            })
            .collect();
        let mut finding = AuditFinding::new(
            FindingKind::SignatureDrift,
            page,
            format!(
                "Documented arguments ({}){} differ from source declarations: {}. Review optional/internal parameters and overloads.",
                names.join(", "),
                if variadic { ", ..." } else { "" },
                declarations.join("; ")
            ),
        );
        finding.sources = candidates
            .iter()
            .map(|(file, def)| SourceEvidence::symbol(file, def))
            .collect();
        findings.push(finding);
    }
    let rets = wiki_markup::blocks(&function.inner, "rets");
    let documented: Vec<_> = rets
        .first()
        .map(|rets| wiki_markup::outer_blocks(&rets.inner, "ret"))
        .unwrap_or_default();
    let shapes: Option<Vec<_>> = candidates
        .iter()
        .map(|(_, def)| def.contract.as_ref()?.returns.as_ref().cloned())
        .collect();
    if let Some(shapes) = shapes {
        let shapes: Vec<_> = shapes.into_iter().flatten().collect();
        let counts: HashSet<_> = shapes.iter().map(Vec::len).collect();
        let mut problem = None;
        if !counts.contains(&documented.len()) {
            let mut counts: Vec<_> = counts.into_iter().collect();
            counts.sort();
            problem = Some(format!(
                "Wiki documents {} return values; explicit source paths return {:?} values",
                documented.len(),
                counts
            ));
        } else if !documented.is_empty()
            && shapes.iter().all(|shape| shape.len() == documented.len())
        {
            for (idx, doc) in documented.iter().enumerate() {
                let expected = doc.tag.attrs.get("type").map(String::as_str).unwrap_or("");
                let expected = expected.split('<').next().unwrap_or(expected);
                if !matches!(
                    expected,
                    "string" | "number" | "boolean" | "table" | "function"
                ) {
                    continue;
                }
                let types: Option<HashSet<_>> =
                    shapes.iter().map(|shape| shape[idx].as_deref()).collect();
                if let Some(types) = types {
                    if !types.contains(expected) {
                        problem = Some(format!(
                            "Wiki return #{} is {}; explicit literal returns are {:?}",
                            idx + 1,
                            expected,
                            types
                        ));
                        break;
                    }
                }
            }
        }
        if let Some(problem) = problem {
            let mut finding = AuditFinding::new(
                FindingKind::ReturnDrift,
                page,
                format!(
                    "{}. Review the documented contract; no type or control-flow guess is applied.",
                    problem
                ),
            );
            finding.sources = candidates
                .iter()
                .map(|(file, def)| SourceEvidence::symbol(file, def))
                .collect();
            findings.push(finding);
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast_indexer::FileIndex;
    fn compare(source: &str, docs: &str) -> Vec<AuditFinding> {
        let def = FileIndex::parse_file("lua/test.lua", source)
            .definitions
            .remove(0);
        contract_findings(
            "api.Test",
            &wiki_markup::blocks(docs, "function")[0],
            &[("lua/test.lua".into(), def)],
        )
    }
    #[test]
    fn callback_arguments_do_not_become_outer_parameters_and_overloads_match() {
        let docs = "<function name='Test'><args><arg name='value' type='number'>x</arg><arg name='callback' type='function'><callback><arg name='result' type='string'>x</arg></callback></arg></args></function>";
        assert!(compare("function api.Test(value, callback) end", docs).is_empty());
        let candidates = FileIndex::parse_file(
            "lua/test.lua",
            "function api.Test(value) end function api.Test(value, callback) end",
        )
        .definitions
        .into_iter()
        .map(|def| ("lua/test.lua".into(), def))
        .collect::<Vec<_>>();
        assert!(
            contract_findings(
                "api.Test",
                &wiki_markup::blocks(docs, "function")[0],
                &candidates
            )
            .is_empty()
        );
        assert_eq!(
            normalize_reference("https://wiki.facepunch.com/gmod/Panel%3ASetPos"),
            Some("Panel:SetPos".into())
        );
    }
    #[test]
    fn malformed_tags_include_missing_attributes_duplicates_and_out_of_bounds() {
        let markup = "<function name='Test'><file>lua/test.lua</file><file line='2-L30'>lua/test.lua</file><file line='2-L30'>lua/test.lua</file></function><file line='3'>lua/test.lua";
        let findings = source_markup_findings("Test", markup, |_| Some(10));
        assert_eq!(findings.len(), 4);
        assert!(
            findings
                .iter()
                .any(|finding| finding.message.contains("duplicate"))
        );
    }
    #[test]
    fn references_ignore_code_fragments_external_realms_and_same_page_anchors() {
        let markup = "<page text='ok'>api.Test#anchor</page><page>#local</page><code><page>Missing</page></code><page realm='rust'>Other</page><page>https://example.com/Elsewhere</page><page>~search?q=predicted</page>";
        assert_eq!(page_references(markup), vec![("api.Test".into(), 1)]);
    }
    #[test]
    fn signatures_handle_self_variadic_arguments_and_clear_return_mismatches() {
        let docs = "<function name='Test' type='classfunc'><args><arg name='value' type='number'>x</arg></args><rets><ret type='number'>x</ret></rets></function>";
        assert!(compare("function api:Test(value) return 1 end", docs).is_empty());
        let findings = compare(
            "function api:Test(other, extra) return true, false end",
            docs,
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == FindingKind::SignatureDrift)
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == FindingKind::ReturnDrift)
        );
        assert!(
            compare(
                "function api.Test(self, value, ...) return Other() end",
                docs
            )
            .is_empty()
        );
    }
    #[test]
    fn dynamic_returns_and_intentional_optional_paths_are_not_false_mismatches() {
        let docs = "<function name='Test'><args><arg name='x' type='boolean'>x</arg></args><rets><ret type='number'>x</ret></rets></function>";
        assert!(compare("function api.Test(x) if x then return 1 end end", docs).is_empty());
        assert!(compare("function api.Test(x) return Other() end", docs).is_empty());
    }
}
