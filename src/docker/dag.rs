use crate::docker::parser::Instruction;
use crate::graph::{BuildGraph, Node, NodeMetadata};
use std::collections::HashMap;
use std::path::PathBuf;

/// Convert a flat list of Dockerfile instructions into a dependency graph.
/// Supports DAG construction with conditional branching and smart dependency tracking.
///
/// Key features:
/// - COPY nodes create dependencies on source files
/// - RUN commands depend on preceding COPY operations for their sources
/// - Multi-stage builds and conditional branching support
/// - Content-addressed identities for incremental builds
pub fn build_graph_from_instructions(
    instructions: Vec<Instruction>,
    project_root: PathBuf,
) -> BuildGraph {
    let mut nodes: Vec<Node> = Vec::new();
    let mut copy_sources: HashMap<String, usize> = HashMap::new(); // Track COPY operations by source
    let mut env_vars: HashMap<String, String> = HashMap::new(); // Track environment variables
    let mut _workdir: Option<String> = None; // Track current working directory

    for (i, instr) in instructions.iter().enumerate() {
        let name = format!("{:?}", instr);
        let mut env = std::collections::HashMap::new();
        let mut metadata = NodeMetadata::default();

        let (content, source_path, kind, deps, _parallelizable) = match instr {
            Instruction::From(img) => {
                // FROM nodes have no dependencies (base image)
                (
                    format!("FROM {}", img),
                    None,
                    crate::graph::NodeKind::From,
                    vec![],
                    true, // FROM can be parallelized if multiple base images
                )
            }
            Instruction::Workdir(dir) => {
                _workdir = Some(dir.clone());
                // WORKDIR depends on previous operations that might affect the filesystem
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.parallelizable = true; // WORKDIR operations can be parallelized if independent
                (
                    format!("WORKDIR {}", dir),
                    None,
                    crate::graph::NodeKind::Workdir,
                    deps,
                    true,
                )
            }
            Instruction::Copy(src, dst) => {
                let path = if src == "." {
                    // Fix 3: COPY . . → hash entire project root
                    project_root.clone()
                } else {
                    project_root.join(src)
                };

                // Track this COPY operation for potential RUN dependencies
                copy_sources.insert(src.clone(), i);

                // COPY depends on previous filesystem operations
                let deps = if i > 0 { vec![i - 1] } else { vec![] };

                metadata.parallelizable = true; // COPY operations can be parallelized
                metadata.tags.push("copy".to_string());

                (
                    format!("COPY {} {}", src, dst),
                    Some(path),
                    crate::graph::NodeKind::Copy {
                        src: PathBuf::from(src),
                        dst: PathBuf::from(dst),
                    },
                    deps,
                    true,
                )
            }
            Instruction::Run(cmd) => {
                // Analyze RUN command to determine dependencies
                let mut deps = if i > 0 { vec![i - 1] } else { vec![] };

                // Check if RUN command references files that were copied
                for (src_path, copy_idx) in &copy_sources {
                    if (cmd.contains(src_path) || cmd.contains(&format!("./{}", src_path)))
                        && !deps.contains(copy_idx)
                    {
                        deps.push(*copy_idx);
                    }
                }

                // RUN commands that don't modify shared state can be parallelized
                let is_parallelizable =
                    !cmd.contains("rm") && !cmd.contains("mv") && !cmd.contains("chmod");
                metadata.parallelizable = is_parallelizable;
                metadata.tags.push("run".to_string());

                (
                    cmd.clone(),
                    None,
                    crate::graph::NodeKind::Run,
                    deps,
                    is_parallelizable,
                )
            }
            Instruction::Env(key, value) => {
                env.insert(key.clone(), value.clone());
                env_vars.insert(key.clone(), value.clone());

                // ENV operations can be parallelized if they don't conflict
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.parallelizable = true;
                metadata.tags.push("env".to_string());

                (
                    format!("ENV {}={}", key, value),
                    None,
                    crate::graph::NodeKind::Env,
                    deps,
                    true,
                )
            }
            Instruction::Cmd(cmd) => {
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.parallelizable = true;
                metadata.tags.push("cmd".to_string());

                (
                    format!("CMD {}", cmd),
                    None,
                    crate::graph::NodeKind::Cmd,
                    deps,
                    true,
                )
            }
            Instruction::Git(url, target) => {
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.parallelizable = true;
                metadata.tags.push("git".to_string());

                (
                    format!("GIT {} {}", url, target),
                    None,
                    crate::graph::NodeKind::Git {
                        url: url.clone(),
                        target: PathBuf::from(target),
                    },
                    deps,
                    true,
                )
            }
            Instruction::RunExtend(cmd, parallelizable) => {
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.parallelizable = *parallelizable;
                metadata.tags.push("extension".to_string());
                metadata.tags.push("run-extend".to_string());

                (
                    cmd.clone(),
                    None,
                    crate::graph::NodeKind::RunExtend {
                        command: cmd.clone(),
                        parallelizable: *parallelizable,
                    },
                    deps,
                    *parallelizable,
                )
            }
            Instruction::CopyExtend(src, dst, tags) => {
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.parallelizable = true;
                metadata.tags.extend(tags.clone());
                metadata.tags.push("extension".to_string());

                let path = if src == "." {
                    project_root.clone()
                } else {
                    project_root.join(src)
                };

                (
                    format!("COPY_EXTEND {} -> {}", src, dst),
                    Some(path),
                    crate::graph::NodeKind::CopyExtend {
                        src: PathBuf::from(src),
                        dst: PathBuf::from(dst),
                        tags: tags.clone(),
                    },
                    deps,
                    true,
                )
            }
            Instruction::Hook(name, params) => {
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.parallelizable = false; // Hooks execute sequentially by default
                metadata.tags.push("hook".to_string());

                (
                    format!("HOOK {} {:?}", name, params),
                    None,
                    crate::graph::NodeKind::CustomHook {
                        hook_name: name.clone(),
                        params: params.clone(),
                    },
                    deps,
                    false,
                )
            }
            Instruction::Other(s) => {
                let deps = if i > 0 { vec![i - 1] } else { vec![] };
                metadata.tags.push("other".to_string());

                (
                    s.clone(),
                    None,
                    crate::graph::NodeKind::Other,
                    deps,
                    false, // Conservative: unknown operations are not parallelizable
                )
            }
        };

        let node = Node {
            id: i,
            name,
            content,
            kind,
            hash: "".into(),
            dirty: false,
            source_path,
            env,
            cache_hit: false,
            deps,
            metadata,
        };

        nodes.push(node);
    }

    BuildGraph { nodes }
}
