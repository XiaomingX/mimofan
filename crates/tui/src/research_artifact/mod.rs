//! 研究成果物汇总（#750）：对标 open-discovery 的 Repository Artifact Builder。
//!
//! 把一次 initiative（goal_loop / evolve 运行）的产物汇总为单一可复现目录：
//! 含 BRIEF 原文、可运行 setup、代表性正/负结果、provenance。
//! 默认只生成本地目录，不自动发布（--publish 由 #753 的 execpolicy 闸门约束）。
//!
//! 与既有 `crates/tui/src/artifacts` 模块区分：后者是「会话级单条产物元数据索引」，
//! 本模块是「跨回合/跨候选的 initiative 级汇总产物」。

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::repro::Brief;
use crate::repro::ProvenanceRecord;

pub const ARTIFACT_README: &str = "README.md";
pub const ARTIFACT_PROVENANCE: &str = "provenance.json";
pub const INITIATIVES_DIR: &str = "initiatives";

/// 一条被收录的结论/代码块（来自评审 #752 的 accepted claim）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub title: String,
    pub body: String,
    /// 证据强度（与 reviewer 对齐）：strong / medium / weak。
    #[serde(default = "default_strength")]
    pub strength: String,
    /// 支撑证据（如 evaluator 输出路径、测试名）。
    #[serde(default)]
    pub evidence: Vec<String>,
}

fn default_strength() -> String {
    "medium".into()
}

/// 代表性正负结果。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Results {
    #[serde(default)]
    pub positive: Vec<String>,
    #[serde(default)]
    pub negative: Vec<String>,
}

/// 汇总输入。
#[derive(Debug, Clone)]
pub struct ArtifactInput {
    pub brief: Brief,
    pub claims: Vec<Claim>,
    pub results: Results,
    pub provenance: Vec<ProvenanceRecord>,
    /// 复现步骤（setup 命令，纯文本行）。
    pub setup_steps: Vec<String>,
}

impl Default for ArtifactInput {
    fn default() -> Self {
        ArtifactInput {
            brief: Brief::default(),
            claims: Vec::new(),
            results: Results::default(),
            provenance: Vec::new(),
            setup_steps: Vec::new(),
        }
    }
}

impl ArtifactInput {
    /// 渲染 README.md 文本（纯函数，可单测）。
    pub fn render_readme(&self) -> String {
        let mut s = String::new();
        s.push_str("# Research Artifact\n\n");
        s.push_str("## Brief\n\n");
        s.push_str(&self.brief.text.trim());
        s.push('\n');

        if !self.setup_steps.is_empty() {
            s.push_str("\n## Setup\n\n```\n");
            for step in &self.setup_steps {
                s.push_str(step);
                s.push('\n');
            }
            s.push_str("```\n");
        }

        if !self.results.positive.is_empty() || !self.results.negative.is_empty() {
            s.push_str("\n## Results\n\n");
            if !self.results.positive.is_empty() {
                s.push_str("### Positive\n\n");
                for p in &self.results.positive {
                    s.push_str(&format!("- {p}\n"));
                }
            }
            if !self.results.negative.is_empty() {
                s.push_str("\n### Negative\n\n");
                for n in &self.results.negative {
                    s.push_str(&format!("- {n}\n"));
                }
            }
        }

        if !self.claims.is_empty() {
            s.push_str("\n## Claims (accepted)\n\n");
            for c in &self.claims {
                s.push_str(&format!("### {}  _[{}]_\n\n", c.title, c.strength));
                s.push_str(&c.body.trim());
                s.push('\n');
                if !c.evidence.is_empty() {
                    s.push_str("\nEvidence:\n");
                    for e in &c.evidence {
                        s.push_str(&format!("- {e}\n"));
                    }
                }
            }
        }

        s.push_str("\n## Provenance\n\n");
        if self.provenance.is_empty() {
            s.push_str("_no provenance recorded_\n");
        } else {
            s.push_str("| timestamp | turn | model | writes | note |\n");
            s.push_str("|---|---|---|---|---|\n");
            for p in &self.provenance {
                let writes = if p.files_written.is_empty() {
                    "-".to_string()
                } else {
                    p.files_written.join(", ")
                };
                s.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    p.timestamp.format("%Y-%m-%d %H:%M"),
                    p.turn_id,
                    p.model,
                    writes,
                    p.note
                ));
            }
        }
        s.push('\n');
        s
    }

    /// 汇总到 `<root>/initiatives/<initiative_id>/`，写 README.md 与 provenance.json。
    pub fn build(&self, root: &Path, initiative_id: &str) -> Result<PathBuf> {
        let dir = root.join(INITIATIVES_DIR).join(initiative_id);
        std::fs::create_dir_all(&dir)?;
        let readme = dir.join(ARTIFACT_README);
        std::fs::write(&readme, self.render_readme())?;
        let prov = dir.join(ARTIFACT_PROVENANCE);
        std::fs::write(&prov, serde_json::to_string_pretty(&self.provenance)?)?;
        Ok(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repro::ProvenanceRecord;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("mimofan-artifact-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn readme_includes_brief_and_results() {
        let input = ArtifactInput {
            brief: Brief::new("Reproduce X", "goal"),
            results: Results {
                positive: vec!["+30.4% speed".into()],
                negative: vec!["no gain on long seq".into()],
            },
            ..Default::default()
        };
        let md = input.render_readme();
        assert!(md.contains("Reproduce X"));
        assert!(md.contains("+30.4% speed"));
        assert!(md.contains("no gain on long seq"));
    }

    #[test]
    fn readme_lists_claims_with_strength() {
        let input = ArtifactInput {
            brief: Brief::new("X", "goal"),
            claims: vec![Claim {
                title: "fixed 2-token lookup faster".into(),
                body: "greedy-equivalent tokens".into(),
                strength: "strong".into(),
                evidence: vec!["evaluator speedup=1.304".into()],
            }],
            ..Default::default()
        };
        let md = input.render_readme();
        assert!(md.contains("fixed 2-token lookup faster"));
        assert!(md.contains("[strong]"));
        assert!(md.contains("evaluator speedup=1.304"));
    }

    #[test]
    fn build_writes_readme_and_provenance() {
        let input = ArtifactInput {
            brief: Brief::new("X", "goal"),
            provenance: vec![ProvenanceRecord {
                turn_id: "turn-1".into(),
                model: "glm-5".into(),
                files_written: vec!["a.py".into()],
                note: "impl".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let d = tmp();
        let dir = input.build(&d, "init-1").unwrap();
        assert!(dir.join("README.md").exists());
        assert!(dir.join("provenance.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
