use crate::ast_indexer::Realm;

#[derive(Debug, Clone)]
pub struct SourceTagMatch {
    pub current_line_attr: String,
    pub file_path: String,
    pub func_name: Option<String>,
    pub func_parent: Option<String>,
    pub realm: Option<Realm>,
    pub raw_realm: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscrepancyKind {
    LineRangeDrift,
    WrongFile {
        original_file: String,
        target_file: String,
    },
    MissingSourceTag {
        target_file: String,
    },
    FileNotFound {
        original_file: String,
    },
    RealmMismatch {
        wiki_realm: String,
        repo_realm: String,
    },
    MissingRealmTag {
        repo_realm: String,
    },
}

#[derive(Debug, Clone)]
pub struct Discrepancy {
    pub page_address: String,
    pub file_path: String,
    pub symbol_name: String,
    pub current_line_attr: String,
    pub expected_line_attr: String,
    pub expected_start_line: usize,
    pub expected_end_line: usize,
    pub kind: DiscrepancyKind,
}

#[derive(Debug, Clone)]
pub struct PageDiffResult {
    pub page_address: String,
    pub discrepancies: Vec<Discrepancy>,
    pub unresolved_sources: Vec<String>,
    pub findings: Vec<crate::audit_checks::AuditFinding>,
}

pub struct DiffEngine;

impl DiffEngine {
    /// Extracts the declared `<realm>` tag from markup, if present.
    pub fn extract_realm(markup: &str) -> (Option<Realm>, Option<String>) {
        crate::wiki_markup::blocks(markup, "realm")
            .first()
            .map_or((None, None), |block| {
                let raw = block.inner.trim().to_string();
                (Realm::parse_wiki_str(&raw), Some(raw))
            })
    }

    pub fn has_source_markup(markup: &str) -> bool {
        crate::wiki_markup::tags(markup)
            .iter()
            .any(|tag| tag.name == "file" && !tag.closing)
    }

    /// Associate every real source tag with its own function block, including
    /// missing attributes and source tags outside function blocks.
    pub fn extract_tags(markup: &str, address: &str) -> Vec<SourceTagMatch> {
        let functions = crate::wiki_markup::blocks(markup, "function");
        let (fallback_parent, fallback_name) = parse_address_parts(address);
        crate::wiki_markup::blocks(markup, "file")
            .into_iter()
            .map(|file| {
                let context = functions.iter().find(|function| {
                    function.tag.start < file.tag.start && file.end <= function.end
                });
                let (realm, raw_realm) =
                    Self::extract_realm(context.map_or(markup, |function| function.inner.as_str()));
                SourceTagMatch {
                    current_line_attr: file.tag.attrs.get("line").cloned().unwrap_or_default(),
                    file_path: crate::wiki_markup::decode(file.inner.trim()),
                    func_name: context
                        .and_then(|function| function.tag.attrs.get("name"))
                        .cloned()
                        .or(Some(fallback_name.clone())),
                    func_parent: context
                        .and_then(|function| function.tag.attrs.get("parent"))
                        .cloned()
                        .or(fallback_parent.clone()),
                    realm,
                    raw_realm,
                }
            })
            .collect()
    }
}

pub fn parse_address_parts(address: &str) -> (Option<String>, String) {
    if let Some(pos) = address.rfind(['.', ':']) {
        (
            Some(address[..pos].to_string()),
            address[pos + 1..].to_string(),
        )
    } else {
        (None, address.to_string())
    }
}
