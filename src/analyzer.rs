use crate::ast_indexer::{
    FileIndex, Realm, SymbolDefinition, normalize_symbol_query, same_symbol_parent,
};
use crate::audit_checks::{AuditFinding, FindingKind, SourceEvidence};
use crate::diff_engine::{
    DiffEngine, Discrepancy, DiscrepancyKind, PageDiffResult, parse_address_parts,
};
use crate::git_repo::GmodRepo;
use crate::load_graph::LoadGraph;
use crate::wiki_client::LinkResolution;
use crate::wiki_client::WikiClient;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub enum CheckPageOutcome {
    NotFound,
    NoMatchingLuaSource {
        title: String,
        findings: Vec<AuditFinding>,
    },
    ExcludedSource {
        title: String,
    },
    ValidSource {
        result: PageDiffResult,
    },
}

pub struct Analyzer {
    pub quality: std::collections::BTreeMap<String, crate::documentation_quality::Rating>,
    pub repo: GmodRepo,
    pub wiki_client: WikiClient,
    pub realm_checked: usize,
    pub realm_unresolved: usize,
    pub menu_symbols_excluded: usize,
    pub menu_files_excluded: usize,
    pub metamethod_candidates_excluded: usize,
    file_cache: HashMap<PathBuf, FileIndex>,
    repo_symbols_indexed: bool,
    /// Map of lowercase symbol query -> Vec<(file_path, SymbolDefinition)>
    repo_symbol_map: HashMap<String, Vec<(String, SymbolDefinition)>>,
    class_bases: HashMap<String, String>,
    registered_controls: HashMap<String, SourceEvidence>,
    source_texts: HashMap<String, String>,
    pinned: bool,
    pub load_graph: LoadGraph,
    pub signature_comparisons: usize,
    pub return_comparisons: usize,
    pub load_comparisons: usize,
}

impl Analyzer {
    // Report excerpts use the same immutable texts as analysis and pinned evidence.
    // Revisit if local, uncommitted audits become supported; those need distinct provenance.
    pub fn report_excerpt(&self, evidence: &SourceEvidence) -> Option<serde_json::Value> {
        if !self.pinned || evidence.start == 0 {
            return None;
        }
        let text = self.source_texts.get(&evidence.file)?;
        let lines: Vec<_> = text
            .lines()
            .skip(evidence.start - 1)
            .take((evidence.end.saturating_sub(evidence.start) + 1).min(80))
            .collect();
        if lines.is_empty() {
            return None;
        }
        Some(serde_json::json!({"start": evidence.start, "lines": lines,
            "truncated": evidence.end.saturating_sub(evidence.start) >= 80}))
    }

    pub fn new(repo: GmodRepo, wiki_client: WikiClient) -> Self {
        Self {
            quality: Default::default(),
            repo,
            wiki_client,
            realm_checked: 0,
            realm_unresolved: 0,
            menu_symbols_excluded: 0,
            menu_files_excluded: 0,
            metamethod_candidates_excluded: 0,
            file_cache: HashMap::new(),
            repo_symbols_indexed: false,
            repo_symbol_map: HashMap::new(),
            class_bases: HashMap::new(),
            registered_controls: HashMap::new(),
            source_texts: HashMap::new(),
            pinned: false,
            load_graph: LoadGraph::default(),
            signature_comparisons: 0,
            return_comparisons: 0,
            load_comparisons: 0,
        }
    }

    pub fn from_snapshot(
        repo: GmodRepo,
        wiki_client: WikiClient,
        snapshot: &crate::git_repo::SourceSnapshot,
    ) -> Result<Self, String> {
        let sources = repo.snapshot_sources(snapshot)?;
        let mut analyzer = Self::new(repo, wiki_client);
        analyzer.pinned = true;
        for (path, bytes) in sources {
            if !source_is_included(&path) {
                // Keep menu load roots as evidence for helpers outside menu directories.
                // They are not documentation candidates or source-check inputs.
                if menu_source_path(&path) {
                    analyzer.menu_files_excluded += 1;
                    let source = String::from_utf8(bytes).map_err(|error| {
                        format!("Cannot read menu load evidence {path}: {error}")
                    })?;
                    analyzer.source_texts.insert(path, source);
                }
                continue;
            }
            let source = String::from_utf8(bytes)
                .map_err(|error| format!("Cannot index committed source {path}: {error}"))?;
            let index = FileIndex::parse_file_checked(&path, &source)?;
            analyzer
                .file_cache
                .insert(analyzer.repo.root_dir.join("garrysmod").join(&path), index);
            analyzer.source_texts.insert(path, source);
        }
        analyzer.ensure_repo_symbols_indexed()?;
        Ok(analyzer)
    }

    /// Indexes all Lua files in the repository to build a global symbol map.
    pub fn ensure_repo_symbols_indexed(&mut self) -> Result<(), String> {
        if self.repo_symbols_indexed {
            return Ok(());
        }

        let garrysmod_dir = self.repo.root_dir.join("garrysmod");
        let search_dirs = [garrysmod_dir.join("lua"), garrysmod_dir.join("gamemodes")];

        if !self.pinned {
            let mut paths = Vec::new();
            for dir in &search_dirs {
                if !dir.exists() {
                    continue;
                }
                for entry in WalkDir::new(dir).into_iter().filter_entry(|entry| {
                    entry
                        .path()
                        .strip_prefix(&garrysmod_dir)
                        .ok()
                        .is_none_or(|path| source_is_included(&path.to_string_lossy()))
                }) {
                    let entry = entry
                        .map_err(|error| format!("Incomplete repository traversal: {error}"))?;
                    if entry.path().extension().is_some_and(|ext| ext == "lua") {
                        paths.push(entry.into_path());
                    }
                }
            }
            if paths.is_empty() {
                return Err(
                    "No included Lua sources found; repository coverage is incomplete".into(),
                );
            }
            paths.sort();
            for path in paths {
                self.get_or_parse_file(&path)?;
            }
        }
        self.repo_symbol_map.clear();
        self.menu_symbols_excluded = 0;
        self.load_graph = LoadGraph::build(&self.source_texts);
        self.class_bases.clear();
        self.registered_controls.clear();
        let mut indexes: Vec<_> = self.file_cache.values().collect();
        indexes.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        for index in indexes {
            self.class_bases.extend(index.class_bases.clone());
            for (name, &(start, end)) in &index.control_lines {
                self.registered_controls.insert(
                    name.clone(),
                    SourceEvidence {
                        file: index.relative_path.clone(),
                        start,
                        end,
                        label: format!("VGUI control registration: {name}"),
                    },
                );
            }
            let rel_path = index.relative_path.clone();

            for def in index.definitions.iter().cloned() {
                if self.menu_only_definition(&rel_path, &def) {
                    self.menu_symbols_excluded += 1;
                    continue;
                }
                let full_lower = def.full_name.to_lowercase();
                self.repo_symbol_map
                    .entry(full_lower)
                    .or_default()
                    .push((rel_path.clone(), def.clone()));

                if let Some(ref parent) = def.parent {
                    let parent_base_lower = format!("{}.{}", parent, def.base_name).to_lowercase();
                    if parent_base_lower != def.full_name.to_lowercase() {
                        self.repo_symbol_map
                            .entry(parent_base_lower)
                            .or_default()
                            .push((rel_path.clone(), def.clone()));
                    }
                }

                let base_lower = def.base_name.to_lowercase();
                self.repo_symbol_map
                    .entry(base_lower)
                    .or_default()
                    .push((rel_path.clone(), def));
            }
        }

        self.metamethod_candidates_excluded = self
            .repo_symbol_map
            .values()
            .flatten()
            .filter(|(_, def)| {
                is_metamethod(def.parent.as_deref(), &def.base_name) && def.full_name.contains(':')
            })
            .map(|(file, def)| (file, def.source_start_offset))
            .collect::<std::collections::HashSet<_>>()
            .len();
        self.repo_symbols_indexed = true;
        Ok(())
    }

    // This policy owns menu classification across discovery, source and contract checks.
    // Explicit game-realm evidence outranks a menu-only observed load trace: static
    // tracing cannot prove the absence of other load paths. Revisit if menu modding
    // becomes supported, or a complete runtime load model replaces static evidence.
    fn menu_only_definition(&self, file: &str, def: &SymbolDefinition) -> bool {
        def.realm == Realm::Menu
            || (def.realm == Realm::Unknown && self.load_graph.symbol_realms(file, def) == 4)
    }

    /// Searches the repository for a symbol across all indexed Lua files, with optional realm preference.
    pub fn find_symbol_in_repo(
        &mut self,
        func_name: &str,
        parent: Option<&str>,
        preferred_realm: Option<Realm>,
    ) -> Result<Option<(String, SymbolDefinition)>, String> {
        self.ensure_repo_symbols_indexed()?;

        let (query_parent, query_name) = normalize_symbol_query(func_name, parent);
        let key = query_parent.as_ref().map_or_else(
            || query_name.to_lowercase(),
            |parent| format!("{}.{}", parent, query_name).to_lowercase(),
        );
        let Some(matches) = self.repo_symbol_map.get(&key) else {
            return Ok(None);
        };
        // Lua identifiers are case-sensitive. A base-name index must never match a
        // foreign class or a local object merely because its method has the same name.
        let matches: Vec<_> = matches
            .iter()
            .filter(|(_, def)| {
                !def.source_only
                    && same_symbol_parent(def.parent.as_deref(), query_parent.as_deref())
                    && def.base_name == query_name
            })
            .collect();
        let best = preferred_realm
            .and_then(|realm| {
                matches
                    .iter()
                    .find(|(_, def)| def.realm == realm)
                    .or_else(|| {
                        matches
                            .iter()
                            .find(|(_, def)| def.realm.is_compatible_with(realm))
                    })
            })
            .or_else(|| matches.first());
        Ok(best.map(|found| (*found).clone()))
    }

    /// Retrieves or parses the FileIndex for the given file path.
    pub fn get_or_parse_file(&mut self, abs_path: &Path) -> Result<&FileIndex, String> {
        if self
            .repo
            .to_wiki_relative_path(abs_path)
            .is_some_and(|path| !source_is_included(&path))
        {
            return Err("Sandbox and TTT sources are excluded from this audit".into());
        }
        if !self.file_cache.contains_key(abs_path) {
            if self.pinned {
                return Err("Source is not part of the recorded revision".into());
            }
            let content = fs::read_to_string(abs_path)
                .map_err(|e| format!("Failed to read file {}: {}", abs_path.display(), e))?;
            let rel_wiki_path = self
                .repo
                .to_wiki_relative_path(abs_path)
                .unwrap_or_else(|| abs_path.to_string_lossy().to_string());
            let index = FileIndex::parse_file_checked(&rel_wiki_path, &content)?;
            self.source_texts.insert(rel_wiki_path, content);
            self.file_cache.insert(abs_path.to_path_buf(), index);
        }
        Ok(self.file_cache.get(abs_path).unwrap())
    }

    /// Fetches the official ~pagelist from wiki.facepunch.com and filters for candidate Lua pages.
    pub fn get_candidate_pagelist_pages(
        &mut self,
        prefix_filter: Option<&str>,
    ) -> Result<Vec<String>, String> {
        let pagelist = self.wiki_client.fetch_pagelist()?;
        println!(
            "Fetched official ~pagelist ({} total pages across wiki).",
            pagelist.len()
        );

        let known_prefixes = [
            "Global.",
            "table.",
            "string.",
            "math.",
            "util.",
            "file.",
            "net.",
            "hook.",
            "vgui.",
            "draw.",
            "surface.",
            "http.",
            "coroutine.",
            "bit.",
            "debug.",
            "cam.",
            "render.",
            "cvars.",
            "concommand.",
            "sound.",
            "team.",
            "timer.",
            "undo.",
            "weapons.",
            "effects.",
            "derma.",
            "halo.",
            "properties.",
            "drive.",
            "markup.",
            "jit.",
            "motionsensor.",
            "steamworks.",
            "video.",
            "input.",
            "gui.",
            "gamemode.",
            "cleanup.",
            "player_manager.",
            "ai_schedule.",
            "ai_task.",
            "player.",
            "ents.",
            "sql.",
            "saverestore.",
            "notification.",
            "menubar.",
            "killicon.",
            "cookie.",
            "controlpanel.",
            "construct.",
            "constraint.",
            "chat.",
            "broadcast.",
        ];

        let mut filtered = Vec::new();
        for item in pagelist {
            if let Some(prefix) = prefix_filter {
                if item.address.starts_with(prefix) {
                    filtered.push(item.address);
                }
            } else {
                let addr = &item.address;
                let is_known_lib = known_prefixes.iter().any(|p| addr.starts_with(p));
                let is_derma = addr.starts_with('D') && (addr.contains('.') || addr.contains(':'));
                let is_api = addr.contains(['.', ':']) && is_static_page_address(addr);
                if is_known_lib || is_derma || is_api {
                    filtered.push(item.address);
                }
            }
        }

        // Compare repository symbols with the complete index, including pages excluded
        // from the source audit filter. Report absences as candidates, not proven public APIs.
        let repo_pages = self.scan_repo_symbols()?;
        let missing = missing_page_candidates(repo_pages, prefix_filter, |address| {
            self.documentation_exists(address)
        });
        filtered.extend(missing);
        filtered.sort();
        filtered.dedup();
        Ok(filtered)
    }

    /// Checks a specific wiki page by address (e.g. "table.Copy" or "Global.Color").
    pub fn check_page(&mut self, page_address: &str) -> Result<CheckPageOutcome, String> {
        let mut page_opt = self.wiki_client.fetch_page(page_address)?;
        if page_opt.is_none() {
            match self.wiki_client.resolve_reference(page_address) {
                LinkResolution::Exists(canonical) => {
                    page_opt = self.wiki_client.fetch_page(&canonical)?
                }
                LinkResolution::Missing => {}
                LinkResolution::Unverified(error) => {
                    return Err(format!("Cannot confirm page absence: {}", error));
                }
            }
        }
        let page = match page_opt {
            Some(p) => p,
            None => return Ok(CheckPageOutcome::NotFound),
        };

        if page.markup.is_empty() {
            return Ok(CheckPageOutcome::NoMatchingLuaSource {
                title: page.title,
                findings: Vec::new(),
            });
        }

        let all_tags = DiffEngine::extract_tags(&page.markup, &page.address);
        let tags: Vec<_> = all_tags
            .iter()
            .filter(|tag| source_is_included(&tag.file_path))
            .cloned()
            .collect();
        if !all_tags.is_empty() && tags.is_empty() {
            return Ok(CheckPageOutcome::ExcludedSource { title: page.title });
        }
        let (page_realm, page_raw_realm) = DiffEngine::extract_realm(&page.markup);
        if page_realm == Some(Realm::Menu) {
            return Ok(CheckPageOutcome::ExcludedSource { title: page.title });
        }
        if !tags.is_empty()
            && tags.iter().all(|tag| {
                self.file_cache
                    .values()
                    .find(|index| {
                        index.relative_path == tag.file_path.trim_start_matches("garrysmod/")
                    })
                    .and_then(|index| {
                        index
                            .find_symbol_with_realm(
                                tag.func_name.as_deref().unwrap_or(&page.title),
                                tag.func_parent.as_deref(),
                                None,
                            )
                            .map(|def| (index, def))
                    })
                    .is_some_and(|(index, def)| {
                        self.menu_only_definition(&index.relative_path, def)
                    })
            })
        {
            return Ok(CheckPageOutcome::ExcludedSource { title: page.title });
        }
        let findings = self.audit_page_metadata(&page)?;
        if let Some(rating) = crate::documentation_quality::rate(&page.address, &page.markup) {
            self.quality.insert(page.address.clone(), rating);
        }
        let mut discrepancies = Vec::new();
        let mut unresolved_sources = Vec::new();

        if tags.is_empty() && DiffEngine::has_source_markup(&page.markup) {
            unresolved_sources.push("Source markup is present but cannot be associated with a complete source tag; no missing-tag assumption made".into());
        } else if tags.is_empty() {
            // Case 1: Page has NO <file> tag.
            // Check if this symbol actually exists in our Lua repository (missing source tag)
            // or if it's truly a native C function (not found in repo Lua files).
            let (parent_opt, func_name) = parse_address_parts(&page.address);
            if let Some((target_file, def)) =
                self.find_symbol_in_repo(&func_name, parent_opt.as_deref(), page_realm)?
            {
                discrepancies.push(Discrepancy {
                    page_address: page.address.clone(),
                    file_path: target_file.clone(),
                    symbol_name: def.full_name.clone(),
                    current_line_attr: "MISSING".to_string(),
                    expected_line_attr: def.wiki_line_attr.clone(),
                    expected_start_line: def.start_line,
                    expected_end_line: def.end_line,
                    kind: DiscrepancyKind::MissingSourceTag {
                        target_file: target_file.clone(),
                    },
                });

                // Check realm on newly discovered symbol
                self.check_realm_mismatch(
                    &page,
                    &def,
                    &target_file,
                    page_realm,
                    page_raw_realm.as_deref(),
                    &mut discrepancies,
                );
            } else {
                return Ok(CheckPageOutcome::NoMatchingLuaSource {
                    title: page.title,
                    findings,
                });
            }
        } else {
            // Case 2: Page HAS <file> tag(s)
            for tag in tags {
                let local_path_opt = if self.pinned {
                    crate::git_repo::normalized_source_path(&tag.file_path)
                        .filter(|path| self.source_texts.contains_key(path))
                        .map(|path| self.repo.root_dir.join("garrysmod").join(path))
                } else {
                    self.repo.resolve_file_path(&tag.file_path)
                };
                let func_name = tag.func_name.as_deref().unwrap_or(&page.title);
                let func_parent = tag.func_parent.as_deref();
                let effective_realm = tag.realm.or(page_realm);
                let effective_raw_realm = tag.raw_realm.as_deref().or(page_raw_realm.as_deref());

                if let Some(local_path) = local_path_opt {
                    let index = self.get_or_parse_file(&local_path)?.clone();
                    let symbol_def = index
                        .find_symbol_with_realm(func_name, func_parent, effective_realm)
                        .filter(|def| !self.menu_only_definition(&index.relative_path, def));

                    if let Some(def) = symbol_def {
                        if is_line_attr_different(&tag.current_line_attr, &def) {
                            discrepancies.push(Discrepancy {
                                page_address: page.address.clone(),
                                file_path: tag.file_path.clone(),
                                symbol_name: def.full_name.clone(),
                                current_line_attr: tag.current_line_attr.clone(),
                                expected_line_attr: def.wiki_line_attr.clone(),
                                expected_start_line: def.start_line,
                                expected_end_line: def.end_line,
                                kind: DiscrepancyKind::LineRangeDrift,
                            });
                        }

                        // Check realm mismatch
                        self.check_realm_mismatch(
                            &page,
                            &def,
                            &index.relative_path,
                            effective_realm,
                            effective_raw_realm,
                            &mut discrepancies,
                        );
                    } else {
                        // Symbol NOT in the tagged file! Search the rest of the repository for it!
                        if let Some((actual_file, actual_def)) =
                            self.find_symbol_in_repo(func_name, func_parent, effective_realm)?
                        {
                            discrepancies.push(Discrepancy {
                                page_address: page.address.clone(),
                                file_path: tag.file_path.clone(),
                                symbol_name: actual_def.full_name.clone(),
                                current_line_attr: tag.current_line_attr.clone(),
                                expected_line_attr: actual_def.wiki_line_attr.clone(),
                                expected_start_line: actual_def.start_line,
                                expected_end_line: actual_def.end_line,
                                kind: DiscrepancyKind::WrongFile {
                                    original_file: tag.file_path.clone(),
                                    target_file: actual_file.clone(),
                                },
                            });

                            self.check_realm_mismatch(
                                &page,
                                &actual_def,
                                &actual_file,
                                effective_realm,
                                effective_raw_realm,
                                &mut discrepancies,
                            );
                        } else {
                            let candidates = index
                                .definitions
                                .iter()
                                .filter(|def| def.base_name == func_name)
                                .map(|def| {
                                    format!(
                                        "{} at {} ({})",
                                        def.full_name, def.wiki_line_attr, def.realm
                                    )
                                })
                                .collect::<Vec<_>>();
                            unresolved_sources.push(format!(
                                "Could not match {}{}{} in {}. Same-name definitions or alias assignments in that file: {}. No unambiguous ownership match was established; repository fallback found no matching public identity. A call or constant alone does not establish a function definition.",
                                func_parent.unwrap_or("Global"), ":", func_name, tag.file_path,
                                if candidates.is_empty() { "none indexed".into() } else { candidates.join(", ") }
                            ));
                        }
                    }
                } else {
                    // Tagged file DOES NOT EXIST in the repository!
                    if let Some((actual_file, actual_def)) =
                        self.find_symbol_in_repo(func_name, func_parent, effective_realm)?
                    {
                        discrepancies.push(Discrepancy {
                            page_address: page.address.clone(),
                            file_path: tag.file_path.clone(),
                            symbol_name: actual_def.full_name.clone(),
                            current_line_attr: tag.current_line_attr.clone(),
                            expected_line_attr: actual_def.wiki_line_attr.clone(),
                            expected_start_line: actual_def.start_line,
                            expected_end_line: actual_def.end_line,
                            kind: DiscrepancyKind::WrongFile {
                                original_file: tag.file_path.clone(),
                                target_file: actual_file.clone(),
                            },
                        });

                        self.check_realm_mismatch(
                            &page,
                            &actual_def,
                            &actual_file,
                            effective_realm,
                            effective_raw_realm,
                            &mut discrepancies,
                        );
                    } else {
                        discrepancies.push(Discrepancy {
                            page_address: page.address.clone(),
                            file_path: tag.file_path.clone(),
                            symbol_name: func_name.to_string(),
                            current_line_attr: tag.current_line_attr.clone(),
                            expected_line_attr: "FILE_NOT_FOUND".to_string(),
                            expected_start_line: 0,
                            expected_end_line: 0,
                            kind: DiscrepancyKind::FileNotFound {
                                original_file: tag.file_path.clone(),
                            },
                        });
                    }
                }
            }
        }

        Ok(CheckPageOutcome::ValidSource {
            result: PageDiffResult {
                page_address: page.address,
                discrepancies,
                unresolved_sources,
                findings,
            },
        })
    }

    fn metadata_candidates(
        &self,
        name: &str,
        parent: Option<&str>,
        realm: Option<Realm>,
        files: &[String],
    ) -> Vec<(String, SymbolDefinition)> {
        let (parent, name) = normalize_symbol_query(name, parent);
        let key = parent.as_ref().map_or_else(
            || name.to_lowercase(),
            |parent| format!("{}.{}", parent, name).to_lowercase(),
        );
        let mut seen = std::collections::HashSet::new();
        let mut candidates: Vec<_> = self
            .repo_symbol_map
            .get(&key)
            .into_iter()
            .flatten()
            .filter(|(file, def)| {
                !def.source_only
                    || files.iter().any(|path| {
                        crate::git_repo::normalized_source_path(path).as_deref()
                            == Some(file.as_str())
                    })
            })
            .filter(|(_, def)| {
                same_symbol_parent(def.parent.as_deref(), parent.as_deref())
                    && def.base_name == name
                    && realm.is_none_or(|realm| def.realm.is_compatible_with(realm))
            })
            .filter(|(file, def)| {
                seen.insert((file.clone(), def.source_start_offset, def.source_end_offset))
            })
            .cloned()
            .collect();
        if !files.is_empty() {
            let in_files: Vec<_> = candidates
                .iter()
                .filter(|(file, _)| {
                    files.iter().any(|path| {
                        path.replace('\\', "/")
                            .trim_start_matches("garrysmod/")
                            .eq_ignore_ascii_case(file)
                    })
                })
                .cloned()
                .collect();
            if !in_files.is_empty() {
                candidates = in_files;
            }
        }
        if candidates.is_empty() {
            for file in files {
                let normalized = file.replace('\\', "/");
                let normalized = normalized.trim_start_matches("garrysmod/");
                if let Some(index) = self
                    .file_cache
                    .values()
                    .find(|index| index.relative_path == normalized)
                {
                    if let Some(def) = index.find_symbol_with_realm(&name, parent.as_deref(), realm)
                    {
                        if !self.menu_only_definition(normalized, def) {
                            candidates.push((index.relative_path.clone(), def.clone()));
                        }
                    }
                }
            }
        }
        candidates
    }

    pub fn discrepancy_evidence(&self, discrepancy: &Discrepancy) -> Option<SourceEvidence> {
        let files = match &discrepancy.kind {
            DiscrepancyKind::WrongFile { target_file, .. }
            | DiscrepancyKind::MissingSourceTag { target_file } => vec![target_file.clone()],
            _ => vec![discrepancy.file_path.clone()],
        };
        let candidates = self.metadata_candidates(&discrepancy.symbol_name, None, None, &files);
        candidates
            .iter()
            .find(|(file, def)| {
                files.iter().any(|path| {
                    crate::git_repo::normalized_source_path(path).as_deref() == Some(file.as_str())
                }) && def.start_line == discrepancy.expected_start_line
                    && def.end_line == discrepancy.expected_end_line
            })
            .map(|(file, def)| SourceEvidence::symbol(file, def))
    }

    fn audit_page_metadata(
        &mut self,
        page: &crate::wiki_client::WikiPage,
    ) -> Result<Vec<AuditFinding>, String> {
        self.ensure_repo_symbols_indexed()?;
        let file_count = |file: &str| {
            let normalized = file.replace('\\', "/");
            let normalized = normalized
                .trim_start_matches('/')
                .trim_start_matches("garrysmod/");
            self.source_texts
                .iter()
                .find(|(path, _)| path.eq_ignore_ascii_case(normalized))
                .map(|(_, source)| source.lines().count())
        };
        let mut findings =
            crate::audit_checks::source_markup_findings(&page.address, &page.markup, file_count);
        for (target, line) in crate::audit_checks::page_references(&page.markup) {
            if target == page.address {
                continue;
            }
            let result = self.wiki_client.resolve_reference(&target);
            let (kind, message) = match result {
                LinkResolution::Exists(_) => continue,
                LinkResolution::Missing => (
                    FindingKind::BrokenLink,
                    format!(
                        "Reference to {} has no indexed target or live page after redirect/alias checks",
                        target
                    ),
                ),
                LinkResolution::Unverified(error) => (
                    FindingKind::UnverifiedLink,
                    format!("Reference to {} could not be verified: {}", target, error),
                ),
            };
            let mut finding = AuditFinding::new(kind, &page.address, message);
            finding.wiki_line = Some(line);
            finding.target = Some(target);
            findings.push(finding);
        }
        let functions = crate::wiki_markup::blocks(&page.markup, "function");
        if !functions.is_empty()
            && crate::wiki_markup::blocks(&page.markup, "example").is_empty()
            && functions.iter().any(|function| {
                let name = function
                    .tag
                    .attrs
                    .get("name")
                    .map(String::as_str)
                    .unwrap_or("");
                let parent = function.tag.attrs.get("parent").map(String::as_str);
                !name.is_empty()
                    && !is_metamethod(parent, name)
                    && function
                        .tag
                        .attrs
                        .get("type")
                        .is_none_or(|kind| !matches!(kind.as_str(), "hook" | "event"))
                    && DiffEngine::extract_realm(&function.inner).0 != Some(Realm::Menu)
            })
        {
            findings.push(AuditFinding::new(FindingKind::ExampleOpportunity, &page.address,
                "No example block is present. Consider a short snippet with its execution realm, setup, expected output, and cleanup; this is a contribution opportunity, not a documentation error.".into()));
        }
        for function in &functions {
            let Some(name) = function
                .tag
                .attrs
                .get("name")
                .filter(|name| !name.trim().is_empty())
            else {
                let mut finding = AuditFinding::new(
                    FindingKind::IdentityConflict,
                    &page.address,
                    "Function metadata has no function name".into(),
                );
                finding.wiki_line = Some(crate::wiki_markup::line_at(
                    &page.markup,
                    function.tag.start,
                ));
                findings.push(finding);
                continue;
            };
            let fallback = parse_address_parts(&page.address).0;
            let parent = function
                .tag
                .attrs
                .get("parent")
                .or(fallback.as_ref())
                .map(String::as_str);
            let (realm, _) = DiffEngine::extract_realm(&function.inner);
            let files: Vec<_> = crate::wiki_markup::blocks(&function.inner, "file")
                .into_iter()
                .map(|file| crate::wiki_markup::decode(file.inner.trim()))
                .filter(|file| source_is_included(file))
                .collect();
            let candidates = self.metadata_candidates(name, parent, realm, &files);
            let declared = normalize_symbol_query(name, parent);
            let identity_address = page
                .address
                .strip_suffix("(legacy)")
                .or_else(|| page.address.strip_suffix("(lowercase)"))
                .unwrap_or(&page.address);
            let addressed = normalize_symbol_query(identity_address, None);
            if (page.address.contains(['.', ':']) || functions.len() == 1)
                && !(declared.1 == addressed.1
                    && same_symbol_parent(declared.0.as_deref(), addressed.0.as_deref()))
            {
                let address_defs = self.metadata_candidates(&page.address, None, realm, &[]);
                // Canonical wiki addresses can retain legacy casing while their
                // title explicitly names the actual API (e.g. table.foreach).
                let titled = normalize_symbol_query(&page.title, None);
                let title_alias = declared == titled
                    && declared.1.eq_ignore_ascii_case(&addressed.1)
                    && (same_symbol_parent(declared.0.as_deref(), addressed.0.as_deref())
                        || matches!(
                            (declared.0.as_deref(), addressed.0.as_deref()),
                            (Some("Color"), Some("COLOR"))
                                | (Some("Entity"), Some("ENTITY"))
                                | (Some("Weapon"), Some("WEAPON"))
                        ));
                let alias = title_alias
                    || candidates.iter().any(|(file, def)| {
                        address_defs.iter().any(|(other_file, other)| {
                            file == other_file
                                && def.source_start_offset == other.source_start_offset
                                && def.source_end_offset == other.source_end_offset
                                && (def.base_name == other.base_name
                                    || (def.contract.is_some() && def.contract == other.contract))
                        })
                    })
                    || (declared.1 == addressed.1
                        && addressed
                            .0
                            .as_ref()
                            .zip(declared.0.as_ref())
                            .is_some_and(|(child, base)| self.inherits_from(child, base)));
                if !alias {
                    let mut finding = AuditFinding::new(
                        FindingKind::IdentityConflict,
                        &page.address,
                        format!(
                            "Function metadata {:?}.{} differs from page identity {}; no matching callable alias was resolved",
                            declared.0, declared.1, page.address
                        ),
                    );
                    finding.sources = candidates
                        .iter()
                        .map(|(file, def)| SourceEvidence::symbol(file, def))
                        .collect();
                    finding.wiki_line = Some(crate::wiki_markup::line_at(
                        &page.markup,
                        function.tag.start,
                    ));
                    findings.push(finding);
                }
            }
            if function
                .tag
                .attrs
                .get("type")
                .is_some_and(|kind| kind == "libraryfunc")
                && !candidates.is_empty()
                && candidates
                    .iter()
                    .all(|(_, def)| def.full_name.contains(':'))
            {
                let mut finding = AuditFinding::new(FindingKind::IdentityConflict, &page.address, "Function is marked libraryfunc, but source resolves to registered/metatable class methods; review the function type".into());
                finding.sources = candidates
                    .iter()
                    .map(|(file, def)| SourceEvidence::symbol(file, def))
                    .collect();
                findings.push(finding);
            }
            if !crate::audit_checks::callback_contract(function)
                && !candidates.is_empty()
                && candidates.iter().all(|(_, def)| def.contract.is_some())
            {
                self.signature_comparisons += 1;
                if candidates
                    .iter()
                    .all(|(_, def)| def.contract.as_ref().unwrap().returns.is_some())
                {
                    self.return_comparisons += 1;
                }
            }
            let mut contracts =
                crate::audit_checks::contract_findings(&page.address, function, &candidates);
            for finding in &mut contracts {
                finding.wiki_line = Some(crate::wiki_markup::line_at(
                    &page.markup,
                    function.tag.start,
                ));
            }
            findings.extend(contracts);
            if let Some(realm) = realm {
                let mut observed = 0;
                let mut evidence = Vec::new();
                for (file, def) in &candidates {
                    let mask = self.load_graph.symbol_realms(file, def);
                    if mask != 0 {
                        observed |= mask;
                        evidence.push(SourceEvidence::symbol(file, def));
                        evidence.extend(self.load_graph.evidence(file, mask));
                    }
                }
                if observed != 0 {
                    self.load_comparisons += 1;
                }
                let declared = match realm {
                    Realm::Unknown | Realm::SharedAndMenu => 7,
                    Realm::Shared => 3,
                    Realm::Server => 1,
                    Realm::Client => 2,
                    Realm::Menu => 4,
                    Realm::ClientAndMenu => 6,
                };
                if observed & !declared & 3 != 0 {
                    let mut finding = AuditFinding::new(
                        FindingKind::LoadRealmConflict,
                        &page.address,
                        format!(
                            "Wiki declares {}; static engine/include paths reach this definition in {}. Review realm coverage; runtime dependencies and dynamic loading remain unproven",
                            realm,
                            crate::load_graph::realm_names(observed)
                        ),
                    );
                    let mut seen = std::collections::HashSet::new();
                    evidence.retain(|source| {
                        seen.insert((
                            source.file.clone(),
                            source.start,
                            source.end,
                            source.label.clone(),
                        ))
                    });
                    finding.sources = evidence;
                    findings.push(finding);
                }
            }
        }
        for finding in &mut findings {
            for source in &mut finding.sources {
                if let Some(canonical) = self
                    .source_texts
                    .keys()
                    .find(|path| path.eq_ignore_ascii_case(&source.file))
                {
                    source.file = canonical.clone();
                }
            }
        }
        Ok(findings)
    }

    fn inherits_from(&self, child: &str, expected: &str) -> bool {
        if !self.class_bases.contains_key(child) {
            return false;
        }
        if matches!(expected, "Panel" | "PANEL") {
            return true;
        }
        let mut current = child;
        let mut seen = std::collections::HashSet::new();
        while seen.insert(current) {
            let Some(base) = self.class_bases.get(current) else {
                break;
            };
            if base == expected {
                return true;
            }
            current = base;
        }
        false
    }

    /// Checks if the repository symbol's realm conflicts with or is missing from the wiki page.
    fn check_realm_mismatch(
        &mut self,
        page: &crate::wiki_client::WikiPage,
        def: &SymbolDefinition,
        source_file: &str,
        wiki_realm: Option<Realm>,
        raw_realm: Option<&str>,
        discrepancies: &mut Vec<Discrepancy>,
    ) {
        // Unrestricted file placement does not prove Shared or Menu availability.
        if def.realm == Realm::Unknown {
            self.realm_unresolved += 1;
            return;
        }
        if wiki_realm.is_none() && raw_realm.is_some() {
            self.realm_unresolved += 1;
            return;
        }
        if def.realm == Realm::Menu {
            return;
        }
        self.realm_checked += 1;
        match (wiki_realm, raw_realm) {
            (None, None) => {
                // Missing realm tag entirely
                discrepancies.push(Discrepancy {
                    page_address: page.address.clone(),
                    file_path: source_file.to_string(),
                    symbol_name: def.full_name.clone(),
                    current_line_attr: "MISSING_REALM".to_string(),
                    expected_line_attr: def.realm.as_wiki_str().to_string(),
                    expected_start_line: def.start_line,
                    expected_end_line: def.end_line,
                    kind: DiscrepancyKind::MissingRealmTag {
                        repo_realm: def.realm.as_wiki_str().to_string(),
                    },
                });
            }
            (Some(w_realm), Some(raw)) => {
                if !def.realm.is_compatible_with(w_realm) {
                    discrepancies.push(Discrepancy {
                        page_address: page.address.clone(),
                        file_path: source_file.to_string(),
                        symbol_name: def.full_name.clone(),
                        current_line_attr: format!("REALM:{}", raw),
                        expected_line_attr: format!("REALM:{}", def.realm.as_wiki_str()),
                        expected_start_line: def.start_line,
                        expected_end_line: def.end_line,
                        kind: DiscrepancyKind::RealmMismatch {
                            wiki_realm: raw.to_string(),
                            repo_realm: def.realm.as_wiki_str().to_string(),
                        },
                    });
                }
            }
            _ => {}
        }
    }

    /// Discovers all wiki pages referenced by scanning Lua files in the repository.
    pub fn scan_repo_symbols(&mut self) -> Result<Vec<String>, String> {
        self.ensure_repo_symbols_indexed()?;
        // Use exactly the same scope-resolved definitions as source-tag verification.
        let mut candidate_pages: Vec<_> = self
            .repo_symbol_map
            .values()
            .flatten()
            .filter(|(_, def)| {
                !def.source_only
                    && !(is_metamethod(def.parent.as_deref(), &def.base_name)
                        && def.full_name.contains(':'))
            })
            .filter_map(|(_, def)| match def.parent.as_deref() {
                Some("ENT" | "SWEP") => None, // per-script prototypes, not named wiki classes
                Some("_G") | None => Some(format!("Global.{}", def.base_name)),
                _ => Some(def.full_name.clone()),
            })
            .collect();
        // A control overview documents the control itself, not its inherited methods.
        // Revisit only if the wiki stops using standalone control-name pages.
        candidate_pages.extend(self.registered_controls.keys().cloned());
        candidate_pages.retain(|address| is_static_page_address(address));
        candidate_pages.sort();
        candidate_pages.dedup();
        Ok(candidate_pages)
    }

    pub fn panel_names(&mut self) -> Result<Vec<String>, String> {
        self.ensure_repo_symbols_indexed()?;
        let mut names: Vec<_> = self
            .class_bases
            .keys()
            .chain(self.class_bases.values())
            .cloned()
            .collect();
        names.extend(["Panel".into(), "PANEL".into()]);
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub fn missing_page_evidence(
        &mut self,
        address: &str,
    ) -> Result<Option<SourceEvidence>, String> {
        self.ensure_repo_symbols_indexed()?;
        if let Some(evidence) = self.registered_controls.get(address) {
            return Ok(Some(evidence.clone()));
        }
        let (parent, name) = parse_address_parts(address);
        Ok(self
            .find_symbol_in_repo(&name, parent.as_deref(), None)?
            .map(|(file, def)| SourceEvidence::symbol(&file, &def)))
    }

    fn documentation_exists(&self, address: &str) -> bool {
        let exists = |page: &str| self.wiki_client.indexed_page_exists(page);
        if documented_address(address, &self.class_bases, exists) {
            return true;
        }
        let (Some(parent), name) = parse_address_parts(address) else {
            return false;
        };
        if !self.class_bases.contains_key(&parent) {
            return false;
        }
        let key = format!("{}.{}", parent, name).to_lowercase();
        let Some(definitions) = self.repo_symbol_map.get(&key) else {
            return false;
        };
        // Compatibility controls often register copies of the very same prototype
        // (e.g. Slider/DNumSlider). Existing docs for that exact function body cover
        // the alias, without treating unrelated same-named methods as equivalent.
        definitions.iter().any(|(file, def)| {
            self.repo_symbol_map
                .values()
                .flatten()
                .any(|(other_file, other)| {
                    other_file == file
                        && other.source_start_offset == def.source_start_offset
                        && other.source_end_offset == def.source_end_offset
                        && other.base_name == def.base_name
                        && other
                            .parent
                            .as_ref()
                            .is_some_and(|p| self.class_bases.contains_key(p))
                        && documented_address(&other.full_name, &self.class_bases, exists)
                })
        })
    }
}

fn documented_address(
    address: &str,
    class_bases: &HashMap<String, String>,
    page_exists: impl Fn(&str) -> bool,
) -> bool {
    let exists = |address: &str| {
        if page_exists(address) {
            return true;
        }
        let (parent, name) = parse_address_parts(address);
        parent.is_some_and(|parent| {
            page_exists(&format!("{}:{}", parent, name))
                || page_exists(&format!("{}.{}", parent, name))
        })
    };
    if exists(address) {
        return true;
    }
    let (Some(mut parent), method) = parse_address_parts(address) else {
        return false;
    };
    if matches!(parent.as_str(), "Panel" | "PANEL") {
        let alternate = if parent == "Panel" { "PANEL" } else { "Panel" };
        if exists(&format!("{}:{}", alternate, method)) {
            return true;
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut registered_panel = false;
    while seen.insert(parent.clone()) {
        let Some(base) = class_bases.get(&parent) else {
            break;
        };
        registered_panel = true;
        if exists(&format!("{}:{}", base, method)) {
            return true;
        }
        parent = base.clone();
    }
    // All registered VGUI controls inherit generic PANEL hooks, even if their
    // intermediate base class is implemented in the engine rather than this repo.
    registered_panel
        && (exists(&format!("PANEL:{}", method)) || exists(&format!("Panel:{}", method)))
}

pub(crate) fn is_metamethod(parent: Option<&str>, name: &str) -> bool {
    parent.is_some_and(|parent| !matches!(parent, "Global" | "_G"))
        && matches!(
            name,
            "__index"
                | "__newindex"
                | "__call"
                | "__tostring"
                | "__add"
                | "__sub"
                | "__mul"
                | "__div"
                | "__mod"
                | "__pow"
                | "__unm"
                | "__concat"
                | "__len"
                | "__eq"
                | "__lt"
                | "__le"
                | "__gc"
                | "__mode"
                | "__metatable"
                | "__pairs"
                | "__ipairs"
        )
}

fn is_static_page_address(address: &str) -> bool {
    address.split(['.', ':']).all(|part| {
        let mut chars = part.chars();
        chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn menu_source_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_lowercase();
    let normalized = normalized
        .trim_start_matches('/')
        .trim_start_matches("./")
        .trim_start_matches("garrysmod/");
    normalized == "lua/menu"
        || normalized.starts_with("lua/menu/")
        || matches!(
            normalized,
            "lua/includes/menu.lua" | "lua/includes/init_menu.lua"
        )
}

pub(crate) fn source_is_included(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_lowercase();
    let normalized = normalized.trim_start_matches('/').trim_start_matches("./");
    let normalized = normalized.strip_prefix("garrysmod/").unwrap_or(normalized);
    if menu_source_path(path) {
        return false;
    }
    let mut components = normalized.split('/');
    !(components.next() == Some("gamemodes")
        && matches!(components.next(), Some("sandbox" | "terrortown" | "ttt")))
}

/// Check all discovered namespaces, even if they have no existing wiki pages.
fn missing_page_candidates(
    addresses: Vec<String>,
    prefix: Option<&str>,
    exists: impl Fn(&str) -> bool,
) -> Vec<String> {
    addresses
        .into_iter()
        .filter(|address| {
            if !is_static_page_address(address) {
                return false;
            }
            if prefix.is_some_and(|prefix| !address.starts_with(prefix)) {
                return false;
            }
            if exists(address) {
                return false;
            }
            if address.strip_prefix("Global.").is_some_and(&exists) {
                return false;
            }
            let alternative = address
                .rsplit_once(':')
                .map(|(p, n)| format!("{}.{}", p, n))
                .or_else(|| {
                    address
                        .rsplit_once('.')
                        .map(|(p, n)| format!("{}:{}", p, n))
                });
            !alternative.is_some_and(|address| exists(&address))
        })
        .collect()
}

/// Helper to compare current wiki line attribute with parsed AST symbol.
fn is_line_attr_different(wiki_attr: &str, def: &SymbolDefinition) -> bool {
    let clean_wiki = wiki_attr.trim();
    let clean_expected = def.wiki_line_attr.trim();

    if clean_wiki == clean_expected {
        return false;
    }

    let parse_range = |s: &str| -> Option<(usize, usize)> {
        let parts: Vec<&str> = s.split("-L").collect();
        if parts.len() == 2 {
            let start = parts[0].parse::<usize>().ok()?;
            let end = parts[1].parse::<usize>().ok()?;
            Some((start, end))
        } else if let Ok(single) = s.parse::<usize>() {
            Some((single, single))
        } else {
            None
        }
    };

    if let (Some((w_start, w_end)), Some((e_start, e_end))) =
        (parse_range(clean_wiki), parse_range(clean_expected))
    {
        w_start != e_start || w_end != e_end
    } else {
        clean_wiki != clean_expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wiki_client::WikiPage;

    #[test]
    fn page_audit_connects_all_six_checks_to_real_source_and_load_evidence() {
        use crate::audit_checks::FindingKind;
        let root =
            std::env::temp_dir().join(format!("metadata-integration-{}", std::process::id()));
        let lua = root.join("garrysmod/lua/includes");
        fs::create_dir_all(&lua).unwrap();
        fs::write(lua.join("init.lua"), "include('test.lua')").unwrap();
        fs::write(
            lua.join("test.lua"),
            "function api.Test(x) return true, false end",
        )
        .unwrap();
        let (base, server) = crate::wiki_client::test_server(vec![(
            "/gmod/MissingTarget?format=json",
            404,
            String::new(),
        )]);
        let mut client = WikiClient::new(&base, &root.join("cache"));
        client.set_test_page_index(&["api.Wrong"]);
        let cached_page = client.cache_test_page("api.Wrong", serde_json::json!({ "title": "Wrong", "address": "api.Wrong", "markup":
                "<function name='Test' parent='api' type='libraryfunc'><realm>Client</realm><file>lua/includes/test.lua</file><args></args><rets><ret type='number'>Output</ret></rets><page>MissingTarget</page><code><page>MissingExample</page></code></function>"
        }));
        let mut analyzer = Analyzer::new(
            GmodRepo {
                root_dir: root.clone(),
            },
            client,
        );
        let CheckPageOutcome::ValidSource { result } = analyzer.check_page("api.Wrong").unwrap()
        else {
            panic!("Source must resolve from function metadata");
        };
        for kind in [
            FindingKind::MalformedSource,
            FindingKind::BrokenLink,
            FindingKind::IdentityConflict,
            FindingKind::SignatureDrift,
            FindingKind::ReturnDrift,
            FindingKind::LoadRealmConflict,
        ] {
            assert!(
                result.findings.iter().any(|finding| finding.kind == kind),
                "Missing {kind:?}: {:?}",
                result.findings
            );
        }
        assert!(
            !result
                .findings
                .iter()
                .any(|finding| finding.target.as_deref() == Some("MissingExample"))
        );
        assert!(
            result
                .findings
                .iter()
                .filter(|finding| matches!(
                    finding.kind,
                    FindingKind::SignatureDrift
                        | FindingKind::ReturnDrift
                        | FindingKind::LoadRealmConflict
                ))
                .all(|finding| !finding.sources.is_empty())
        );
        assert_eq!(server.join().unwrap().len(), 1);
        for file in [lua.join("init.lua"), lua.join("test.lua"), cached_page] {
            fs::remove_file(file).unwrap();
        }
        for dir in [
            lua,
            root.join("garrysmod/lua"),
            root.join("garrysmod"),
            root.join("cache"),
            root,
        ] {
            fs::remove_dir(dir).unwrap();
        }
    }

    #[test]
    fn sandbox_and_ttt_are_excluded_but_base_and_other_lua_are_kept() {
        assert!(!source_is_included("gamemodes/sandbox/gamemode/shared.lua"));
        assert!(!source_is_included(
            "garrysmod/gamemodes/terrortown/gamemode/shared.lua"
        ));
        assert!(!source_is_included(r"GAMEMODES\TTT\gamemode\shared.lua"));
        assert!(source_is_included("gamemodes/base/gamemode/shared.lua"));
        assert!(source_is_included("gamemodes/other/gamemode/shared.lua"));
        assert!(source_is_included("lua/ttt/helpers.lua"));
        assert!(source_is_included("lua/vgui/dnumslider.lua"));
    }

    #[test]
    fn excluded_gamemodes_never_enter_either_symbol_lookup_or_discovery() {
        let root = std::env::temp_dir().join(format!("gamemode-scope-test-{}", std::process::id()));
        let dirs = [
            "garrysmod/lua",
            "garrysmod/gamemodes/base/gamemode",
            "garrysmod/gamemodes/sandbox/gamemode",
            "garrysmod/gamemodes/terrortown/gamemode",
            "garrysmod/gamemodes/other/gamemode",
        ];
        let names = [
            "LuaFunction",
            "BaseFunction",
            "SandboxFunction",
            "TTTFunction",
            "OtherFunction",
        ];
        for (dir, name) in dirs.iter().zip(names) {
            let dir = root.join(dir);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("fixture.lua"), format!("function {}() end", name)).unwrap();
        }
        let mut analyzer = Analyzer::new(
            GmodRepo {
                root_dir: root.clone(),
            },
            WikiClient::new("http://127.0.0.1:1", &root.join("cache")),
        );
        assert_eq!(
            analyzer.scan_repo_symbols().unwrap(),
            vec![
                "Global.BaseFunction",
                "Global.LuaFunction",
                "Global.OtherFunction"
            ]
        );
        assert!(
            analyzer
                .find_symbol_in_repo("BaseFunction", Some("Global"), None)
                .unwrap()
                .is_some()
        );
        assert!(
            analyzer
                .find_symbol_in_repo("SandboxFunction", Some("Global"), None)
                .unwrap()
                .is_none()
        );
        assert!(
            analyzer
                .find_symbol_in_repo("TTTFunction", Some("Global"), None)
                .unwrap()
                .is_none()
        );
        let markup = "<file line=\"1\">gamemodes/sandbox/gamemode/fixture.lua</file>";
        analyzer
            .wiki_client
            .set_test_page_index(&["Global.Excluded"]);
        let cached_page = analyzer.wiki_client.cache_test_page("Global.Excluded", serde_json::json!({ "title": "Excluded", "address": "Global.Excluded", "markup": markup }));
        assert!(matches!(
            analyzer.check_page("Global.Excluded").unwrap(),
            CheckPageOutcome::ExcludedSource { .. }
        ));
        for dir in dirs {
            let dir = root.join(dir);
            fs::remove_file(dir.join("fixture.lua")).unwrap();
            fs::remove_dir(&dir).unwrap();
            if dir.file_name().unwrap() == "gamemode" {
                fs::remove_dir(dir.parent().unwrap()).unwrap();
            }
        }
        fs::remove_file(cached_page).unwrap();
        fs::remove_dir(root.join("cache")).unwrap();
        fs::remove_dir(root.join("garrysmod/gamemodes")).unwrap();
        fs::remove_dir(root.join("garrysmod")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn registered_controls_use_existing_base_and_panel_hook_documentation() {
        let bases = HashMap::from([
            ("DNewPanel".into(), "DImageButton".into()),
            ("DImageButton".into(), "DButton".into()),
        ]);
        let pages = [
            "PANEL:OnDrop",
            "Panel:SizeToContents",
            "DButton:DoClick",
            "Player:Known",
        ];
        let exists = |page: &str| pages.contains(&page);
        assert!(documented_address("DNewPanel:OnDrop", &bases, exists));
        assert!(documented_address("DNewPanel:DoClick", &bases, exists));
        assert!(documented_address(
            "DNewPanel:SizeToContents",
            &bases,
            exists
        ));
        assert!(documented_address("Panel:OnDrop", &bases, exists));
        assert!(!documented_address("DNewPanel:NewMethod", &bases, exists));
        assert!(!documented_address("Button.OnDrop", &bases, exists));
        assert!(!documented_address("Other:Known", &bases, exists));
    }

    #[test]
    fn compatibility_docs_require_the_same_ast_function_not_just_the_same_line() {
        let cache = std::env::temp_dir().join(format!("class-doc-alias-{}", std::process::id()));
        let mut client = WikiClient::new("http://127.0.0.1:1", &cache);
        client.set_test_page_index(&["DOriginal:Known"]);
        let mut analyzer = Analyzer::new(
            GmodRepo {
                root_dir: PathBuf::new(),
            },
            client,
        );
        let index = FileIndex::parse_file(
            "lua/test.lua",
            "local T = {} function T:Known() end vgui.Register('DOriginal', T) vgui.Register('DCompatibility', table.Copy(T)) local Other = {} function Other:Known() end vgui.Register('DUnrelated', Other)",
        );
        analyzer.class_bases = index.class_bases;
        for def in index.definitions {
            let key = format!("{}.{}", def.parent.as_ref().unwrap(), def.base_name).to_lowercase();
            analyzer
                .repo_symbol_map
                .entry(key)
                .or_default()
                .push(("lua/test.lua".into(), def));
        }
        assert!(analyzer.documentation_exists("DCompatibility:Known"));
        assert!(!analyzer.documentation_exists("DUnrelated:Known"));
        fs::remove_dir(cache).unwrap();
    }

    #[test]
    fn nested_function_attributes_resolve_to_the_same_namespace_as_addresses() {
        let index = FileIndex::parse_file("lua/test.lua", "function util.worldpicker.Start() end");
        assert!(
            index
                .find_symbol("worldpicker.Start", Some("util"))
                .is_some()
        );
        assert!(
            index
                .find_symbol("Start", Some("util.worldpicker"))
                .is_some()
        );
        assert!(index.find_symbol("util.worldpicker.Start", None).is_some());
    }

    #[test]
    fn source_matching_never_uses_a_foreign_class_or_local_callback() {
        let index = FileIndex::parse_file(
            "lua/test.lua",
            r#"
            local P = FindMetaTable("Panel")
            function P:OnDrop() end
            function Setup(Button)
                Button.OnDrop = function() end
            end
        "#,
        );
        assert!(index.find_symbol("OnDrop", Some("PANEL")).is_some());
        assert!(
            index
                .find_symbol("OnDrop", Some("UnrelatedClass"))
                .is_none()
        );
        assert!(index.find_symbol("OnDrop", Some("Button")).is_none());
        assert!(index.find_symbol("OnDrop", None).is_none());
    }

    #[test]
    fn discovery_and_source_search_share_only_exported_scope_resolved_symbols() {
        let root = std::env::temp_dir().join(format!("scope-integration-{}", std::process::id()));
        let lua = root.join("garrysmod/lua");
        fs::create_dir_all(&lua).unwrap();
        fs::write(
            lua.join("fixture.lua"),
            r#"
            local Internal = {}
            function Internal.Hidden() end
            function publiclib.Real() end
            function Setup(Button)
                Button.OnDrop = function() end
                local AimAng = Angle()
                AimAng.p = 0
            end
            local P = {}
            function P:NewMethod() end
            vgui.Register("DNewPanel", P, "Panel")
        "#,
        )
        .unwrap();
        let mut analyzer = Analyzer::new(
            GmodRepo {
                root_dir: root.clone(),
            },
            WikiClient::new("http://127.0.0.1:1", &root.join("cache")),
        );
        let addresses = analyzer.scan_repo_symbols().unwrap();
        assert_eq!(
            addresses,
            vec![
                "DNewPanel",
                "DNewPanel:NewMethod",
                "Global.Setup",
                "publiclib.Real"
            ]
        );
        for address in addresses {
            assert!(analyzer.missing_page_evidence(&address).unwrap().is_some());
        }
        assert!(
            analyzer
                .find_symbol_in_repo("OnDrop", Some("Button"), None)
                .unwrap()
                .is_none()
        );
        assert!(
            analyzer
                .find_symbol_in_repo("Hidden", Some("Internal"), None)
                .unwrap()
                .is_none()
        );
        fs::remove_file(lua.join("fixture.lua")).unwrap();
        fs::remove_dir(lua).unwrap();
        fs::remove_dir(root.join("garrysmod")).unwrap();
        fs::remove_dir(root.join("cache")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn dynamic_expressions_are_not_missing_page_addresses() {
        assert!(!is_static_page_address("AddonConflicts[id].files[name]"));
        assert!(!is_static_page_address(
            "(SERVER and SCORE or CLSCORE).MaxStreamLength"
        ));
        assert!(is_static_page_address("Player:NewMethod"));
        assert!(is_static_page_address("newlib.NewFunction"));
    }

    #[test]
    fn missing_pages_include_entirely_absent_namespaces_and_methods() {
        let addresses = vec![
            "newlib.First",
            "newlib.Second",
            "Player:NewMethod",
            "DNewPanel:Paint",
            "Global.Known",
            "Player:Known",
            "util.Known",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let existing = ["Known", "Player.Known", "util:Known"];
        let missing =
            missing_page_candidates(addresses, None, |address| existing.contains(&address));
        assert_eq!(
            missing,
            vec![
                "newlib.First",
                "newlib.Second",
                "Player:NewMethod",
                "DNewPanel:Paint"
            ]
        );
        assert_eq!(
            missing_page_candidates(missing, Some("newlib."), |_| false),
            vec!["newlib.First", "newlib.Second"]
        );
    }

    #[test]
    fn realm_findings_require_evidence_and_handle_whitespace() {
        let cache = std::env::temp_dir().join(format!("realm-test-{}", std::process::id()));
        let mut analyzer = Analyzer::new(
            GmodRepo {
                root_dir: PathBuf::new(),
            },
            WikiClient::new("http://127.0.0.1:1", &cache),
        );
        let mut def = FileIndex::parse_file("lua/server/test.lua", "function Test() end")
            .definitions
            .remove(0);
        let page = WikiPage {
            title: "Test".into(),
            address: "Global.Test".into(),
            markup: "<function name=\"Test\"><realm>  Client  </realm></function>".into(),
            html: String::new(),
            revision_id: None,
        };
        let mut issues = Vec::new();
        analyzer.check_realm_mismatch(
            &page,
            &def,
            "lua/server/test.lua",
            Some(Realm::Client),
            Some("Client"),
            &mut issues,
        );
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].expected_line_attr, "REALM:Server");
        def.realm = Realm::Unknown;
        issues.clear();
        analyzer.check_realm_mismatch(
            &page,
            &def,
            "lua/server/test.lua",
            Some(Realm::Client),
            Some("Client"),
            &mut issues,
        );
        assert!(issues.is_empty());
        def.realm = Realm::Client;
        analyzer.check_realm_mismatch(
            &page,
            &def,
            "lua/server/test.lua",
            Some(Realm::ClientAndMenu),
            Some("Client and Menu"),
            &mut issues,
        );
        assert!(issues.is_empty());
        assert_eq!(analyzer.realm_checked, 2);
        assert_eq!(analyzer.realm_unresolved, 1);
        fs::remove_dir(cache).unwrap();
    }

    #[test]
    fn global_lookup_does_not_resolve_foreign_members() {
        let cache = std::env::temp_dir().join(format!("global-test-{}", std::process::id()));
        let mut analyzer = Analyzer::new(
            GmodRepo {
                root_dir: PathBuf::new(),
            },
            WikiClient::new("http://127.0.0.1:1", &cache),
        );
        analyzer.repo_symbols_indexed = true;
        let index = FileIndex::parse_file(
            "lua/test.lua",
            "function Test() end\nfunction other.Test() end",
        );
        analyzer.repo_symbol_map.insert(
            "test".into(),
            index
                .definitions
                .into_iter()
                .map(|d| ("lua/test.lua".into(), d))
                .collect(),
        );
        let (_, def) = analyzer
            .find_symbol_in_repo("Test", Some("Global"), None)
            .unwrap()
            .unwrap();
        assert_eq!(def.full_name, "Test");
        fs::remove_dir(cache).unwrap();
    }
}
