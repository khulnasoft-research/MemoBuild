use crate::graph::{BuildGraph, NodeKind};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct AstAnalyzer;

impl Default for AstAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl AstAnalyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze_dependencies(&self, graph: &mut BuildGraph, context_dir: &Path) {
        println!("   🔍 Performing AST-based dependency detection...");

        let mut extra_deps = Vec::new();

        for node in &graph.nodes {
            if let NodeKind::Copy { src, .. } = &node.kind {
                let full_src = context_dir.join(src);
                if full_src.exists() && full_src.is_file() {
                    if let Some(ext) = full_src.extension() {
                        let path = full_src.clone();
                        match ext.to_str() {
                            Some("js") | Some("ts") | Some("jsx") | Some("tsx") => {
                                let deps = self.find_js_dependencies(&path);
                                if !deps.is_empty() {
                                    println!(
                                        "      🟢 Found {} hidden dependencies in {:?}",
                                        deps.len(),
                                        src
                                    );
                                    for dep in &deps {
                                        println!("         └─ {}", dep.display());
                                    }
                                    extra_deps.push((node.id, deps));
                                }
                            }
                            Some("rs") => {
                                let deps = self.find_rust_dependencies(&path);
                                if !deps.is_empty() {
                                    println!(
                                        "      🟢 Found {} hidden dependencies in {:?}",
                                        deps.len(),
                                        src
                                    );
                                    for dep in &deps {
                                        println!("         └─ {}", dep.display());
                                    }
                                    extra_deps.push((node.id, deps));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Apply findings to metadata
        for (node_id, deps) in extra_deps {
            if let Some(node) = graph.nodes.get_mut(node_id) {
                for dep in deps {
                    node.metadata.tags.push(format!("deps:{}", dep.display()));
                    node.metadata.extra_source_paths.push(dep);
                }
            }
        }
    }

    fn find_js_dependencies(&self, path: &Path) -> Vec<PathBuf> {
        let content = fs::read_to_string(path).unwrap_or_default();
        let mut deps = HashSet::new();

        // Simple regex-based "AST" (simplified for this layer)
        // In a real world, we'd use a parser like swclib or tree-sitter
        let import_regex =
            regex::Regex::new(r#"(?:import|export).*?from\s+['"](.+?)['"]"#).unwrap();
        let require_regex = regex::Regex::new(r#"require\s*\(\s*['"](.+?)['"]\s*\)"#).unwrap();

        for cap in import_regex.captures_iter(&content) {
            deps.insert(cap[1].to_string());
        }
        for cap in require_regex.captures_iter(&content) {
            deps.insert(cap[1].to_string());
        }

        deps.into_iter()
            .filter(|d| d.starts_with('.')) // Only local files
            .map(|d| {
                let mut p = path.parent().unwrap().join(d);
                if !p.exists() {
                    // Try adding extensions
                    if p.with_extension("js").exists() {
                        p = p.with_extension("js");
                    } else if p.with_extension("ts").exists() {
                        p = p.with_extension("ts");
                    }
                }
                p
            })
            .collect()
    }

    fn find_rust_dependencies(&self, path: &Path) -> Vec<PathBuf> {
        let content = fs::read_to_string(path).unwrap_or_default();
        let mut deps = HashSet::new();

        let mod_regex = regex::Regex::new(r#"mod\s+([a-zA-Z0-9_]+);"#).unwrap();

        for cap in mod_regex.captures_iter(&content) {
            let mod_name = &cap[1];
            let mut p = path.parent().unwrap().join(format!("{}.rs", mod_name));
            if !p.exists() {
                p = path.parent().unwrap().join(mod_name).join("mod.rs");
            }
            if p.exists() {
                deps.insert(p);
            }
        }

        deps.into_iter().collect()
    }
}
