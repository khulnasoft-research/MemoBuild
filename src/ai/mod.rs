pub mod ast;
pub mod ci;
pub mod optimizer;

use crate::env::EnvFingerprint;
use crate::graph::BuildGraph;

pub struct AiLayer {
    pub ast_analyzer: ast::AstAnalyzer,
    pub optimizer: optimizer::BuildOptimizer,
    pub ci_advisor: ci::CiAdvisor,
}

impl Default for AiLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AiLayer {
    pub fn new() -> Self {
        Self {
            ast_analyzer: ast::AstAnalyzer::new(),
            optimizer: optimizer::BuildOptimizer::new(),
            ci_advisor: ci::CiAdvisor::new(),
        }
    }

    /// Run AI-powered analysis on the build graph
    pub fn analyze(
        &self,
        graph: &mut BuildGraph,
        env_fp: &EnvFingerprint,
        context_dir: &std::path::Path,
    ) {
        println!("🤖 AI Layer: Analyzing build graph...");

        // 1. AST-based dependency detection
        self.ast_analyzer.analyze_dependencies(graph, context_dir);

        // 2. Build optimization
        self.optimizer.optimize_graph(graph, env_fp);

        // 3. CI Pipeline advice
        self.ci_advisor.analyze_ci_context();
    }
}
