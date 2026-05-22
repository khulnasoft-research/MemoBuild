use std::env;

pub struct CiAdvisor;

impl Default for CiAdvisor {
    fn default() -> Self {
        Self::new()
    }
}

impl CiAdvisor {
    pub fn new() -> Self {
        Self
    }

    pub fn analyze_ci_context(&self) {
        println!("   📉 Analyzing CI pipeline for self-optimization...");

        let github_actions = env::var("GITHUB_ACTIONS").is_ok();
        let gitlab_ci = env::var("GITLAB_CI").is_ok();

        if github_actions {
            println!("      💡 CI Suggestion (GitHub Actions):");
            if !std::path::Path::new(".github/workflows/memobuild.yml").exists() {
                println!("         - 🚀 Actionable: Run 'memobuild generate-ci' to bootstrap your pipeline.");
            }
            println!(
                "         - Split build and test jobs to utilize multiple runners and reduce TTR."
            );
        } else if gitlab_ci {
            println!("      💡 CI Suggestion (GitLab):");
            println!("         - Use 'artifacts:paths' to cache the .memobuild-cache directory.");
        } else {
            println!("      💡 CI Suggestion: No CI environment detected.");
            println!("         - MemoBuild performs best in CI/CD environments with remote caching enabled.");
        }

        // Check for common bottlenecks
        if let Ok(cores) = std::thread::available_parallelism() {
            if cores.get() < 8 {
                println!("      ⚠️  Throttling Risk: Build server has only {} cores. Consider upgrading to a high-compute runner.", cores);
            }
        }
    }
}
