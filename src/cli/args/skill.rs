use clap::{Args, Subcommand};

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum SkillTarget {
    Codex,
    Claude,
    Agents,
}

const SKILL_AFTER_HELP: &str = "Examples:\n  snow-cli skill install --bundled --target claude\n  snow-cli skill install --bundled --target claude --pack itsm --dry-run\n  snow-cli skill install ./skills/snow-cli --target-dir ./.codex/skills --pack itsm\n  snow-cli skill install https://ewatch.github.io/snow-cli/skills/snow-cli/skill.toml --target codex --all-packs\n\nNotes:\n  - --bundled installs the skill embedded in this snow-cli binary (works offline, matches CLI version).\n  - URL sources point to a skill.toml manifest. Listed files are fetched relative to that manifest URL.\n  - Installation validates manifest paths, declared SHA-256 digests (transfer integrity, not signature authenticity), and symlink safety.\n  - Re-installing prunes unmodified files from previously installed packs that are no longer selected; pass --force to overwrite or remove locally edited files.";
// --- Skill ---

#[derive(Args, Debug)]
#[command(after_help = SKILL_AFTER_HELP)]
pub struct SkillArgs {
    #[command(subcommand)]
    pub command: SkillCommands,
}

#[derive(Subcommand, Debug)]
pub enum SkillCommands {
    /// Install a skill bundle from the embedded bundle, a local path, or a URL-hosted skill.toml manifest
    Install {
        /// Local bundle path, local skill.toml path, file:// URL, or http(s) URL to skill.toml (omit with --bundled)
        #[arg(value_name = "SOURCE")]
        source: Option<String>,

        /// Install the skill bundle embedded in this snow-cli binary (works offline)
        #[arg(long, conflicts_with = "source")]
        bundled: bool,

        /// Preview planned files and actions without writing to disk
        #[arg(long)]
        dry_run: bool,

        /// Overwrite or remove locally modified skill files that differ from the previous install lockfile
        #[arg(long)]
        force: bool,

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
