use crate::graph::Node;
use crate::sandbox::{ExecResult, Sandbox, SandboxEnv};
use anyhow::Result;
use async_trait::async_trait;
use std::process::Command;

pub struct LocalSandbox {
    pub workspace_dir: std::path::PathBuf,
}

impl LocalSandbox {
    pub fn new(workspace_dir: std::path::PathBuf) -> Self {
        Self { workspace_dir }
    }
}

#[async_trait]
impl Sandbox for LocalSandbox {
    async fn prepare(&self, node: &Node) -> Result<SandboxEnv> {
        Ok(SandboxEnv {
            workspace_dir: self.workspace_dir.clone(),
            env_vars: node.env.clone(),
        })
    }

    async fn execute(&self, env: &SandboxEnv, node: &Node) -> Result<ExecResult> {
        let cmd = match &node.kind {
            crate::graph::NodeKind::Run => node.content.clone(),
            crate::graph::NodeKind::RunExtend { command, .. } => command.clone(),
            crate::graph::NodeKind::CustomHook { hook_name, params } => {
                format!("{} {}", hook_name, params.join(" "))
            }
            crate::graph::NodeKind::CopyExtend { src, dst, .. } => {
                // Perform file copy directly in Rust
                let src_path = env.workspace_dir.join(src);
                let dst_path = env.workspace_dir.join(dst);
                if let Some(d) = dst_path.parent() {
                    std::fs::create_dir_all(d)?;
                }

                // Copy directory or file
                if src_path.is_dir() {
                    // Simple recursive copy using fs_extra or similar would be nice,
                    // but for now let's use system command cp -r for simplicity in sh/cmd
                    if cfg!(target_os = "windows") {
                        format!("xcopy /E /I {} {}", src.display(), dst.display())
                    } else {
                        format!("cp -r {} {}", src.display(), dst.display())
                    }
                } else {
                    std::fs::copy(&src_path, &dst_path)?;
                    return Ok(ExecResult {
                        exit_code: 0,
                        stdout: format!("Copied {} to {}", src.display(), dst.display())
                            .into_bytes(),
                        stderr: Vec::new(),
                    });
                }
            }
            _ => {
                // For non-RUN nodes, we simulate success and return a metadata-based artifact
                return Ok(ExecResult {
                    exit_code: 0,
                    stdout: format!("Artifact for {}", node.name).into_bytes(),
                    stderr: Vec::new(),
                });
            }
        };

        let output = if cfg!(target_os = "windows") {
            Command::new("cmd")
                .arg("/C")
                .arg(cmd)
                .envs(&env.env_vars)
                .current_dir(&env.workspace_dir)
                .output()?
        } else {
            Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .envs(&env.env_vars)
                .current_dir(&env.workspace_dir)
                .output()?
        };

        Ok(ExecResult {
            exit_code: output.status.code().unwrap_or(1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    async fn cleanup(&self, _env: &SandboxEnv) -> Result<()> {
        Ok(())
    }
}
