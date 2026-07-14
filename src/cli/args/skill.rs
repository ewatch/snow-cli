use clap::{Args, Subcommand};

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SkillTarget {
    Codex,
    Claude,
    Agents,
}

const SKILL_AFTER_HELP: &str = "Examples:\n  snow-cli skill install ./skills/snow-cli --target-dir ./.codex/skills --pack table-api\n  snow-cli skill install https://example.com/snow-cli/skill.toml --target codex --all-packs\n\nNotes:\n  - URL sources point to a skill.toml manifest. Listed files are fetched relative to that manifest URL.\n  - Installation validates manifest paths, declared digests, and symlink safety. snow-cli does not run skill scanners or LLM evals.";
// --- Skill ---

#[derive(Args, Debug)]
#[command(after_help = SKILL_AFTER_HELP)]
pub struct SkillArgs {
    #[command(subcommand)]
    pub command: SkillCommands,
}

#[derive(Subcommand, Debug)]
pub enum SkillCommands {
    /// Install a skill bundle from a local path or URL-hosted skill.toml manifest
    Install {
        /// Local bundle path, local skill.toml path, file:// URL, or http(s) URL to skill.toml
        source: String,

        /// Install root directory. The skill is installed under <target-dir>/<skill-name>
        #[arg(long)]
        target_dir: Option<std::path::PathBuf>,

        /// Known agent target root (codex, claude, or agents). Use --target-dir for custom paths
        #[arg(long, value_enum)]
        target: Option<SkillTarget>,

        /// Override the installed directory name
        #[arg(long)]
        name: Option<String>,

        /// Install a specific pack under packs/<name>. Repeat to install several packs
        #[arg(long)]
        pack: Vec<String>,

        /// Install every pack declared by the bundle
        #[arg(long)]
        all_packs: bool,
    },
}
