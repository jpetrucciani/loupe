use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "loupe",
    version,
    about = "Semantic Unix filters for System One decision models"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, env = "LOUPE_PROFILE", global = true)]
    pub profile: Option<String>,
    #[arg(long, env = "LOUPE_URL", global = true)]
    pub url: Option<String>,
    #[arg(long, env = "LOUPE_PATH", global = true)]
    pub path: Option<String>,
    /// Request/response format: systemone (default) or decisions.
    #[arg(long, env = "LOUPE_API_FORMAT", global = true)]
    pub api_format: Option<loupe_core::wire::ApiFormat>,
    #[arg(short = 'm', long, env = "LOUPE_MODEL", global = true)]
    pub model: Option<String>,
    #[arg(short = 'j', long, env = "LOUPE_CONCURRENCY", global = true)]
    pub concurrency: Option<usize>,
    #[arg(short = 'q', long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,
    #[arg(short = 'v', long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub(crate) fn validate_global_flags(&self) -> Result<(), clap::Error> {
        if self.quiet && self.verbose > 0 {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--quiet cannot be used with --verbose",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Serve read-only classification and selection tools over MCP stdio.
    Mcp {
        /// Overall deadline per inference tool call.
        #[arg(long, default_value = "30s")]
        deadline: String,
        /// Exact input tokens per tool call, charging every retry.
        #[arg(long)]
        budget: Option<u64>,
    },
    /// Review a deferred queue offline, capturing labels and emitting corrections.
    Review {
        file: PathBuf,
        #[arg(long)]
        labels_dir: Option<PathBuf>,
    },
    /// Test one whole state: exit 0 yes, 1 no, 2 uncertain.
    Is(Classify),
    /// Preserve units in the yes bucket.
    Keep(Classify),
    /// Preserve units in the no bucket.
    Drop(Classify),
    /// Annotate each unit with a choice label.
    Tag(Classify),
    /// Send each choice-labeled unit to a confined file or declared process.
    Route(Route),
    /// Annotate each unit with its expected score.
    Rate(Classify),
    /// Stable ascending score order, requiring finite input.
    Sort {
        #[command(flatten)]
        options: Classify,
        #[arg(short = 'r', long)]
        reverse: bool,
    },
    /// Histogram of choice labels or yes/no/uncertain decisions.
    Count(Classify),
    /// Evaluate all questions in a TOML/JSON file against one state.
    Ask(Classify),
    /// Evaluate joint Noul/Score features per row, as CSV or JSONL.
    Featurize(Classify),
    /// Decompose a decision into an auditable weighted Noul/Score factor index.
    Explain(Classify),
    /// Stream semantic matches against a finite indexed right-side table.
    Join(Join),
    /// Cluster duplicate pairs after finite input, keeping a representative.
    Dedupe(Dedupe),
    /// Run declared actions on streaming Noul or Choice decisions.
    On(On),
    /// Install or run trusted, budgeted Git pre-commit / commit-msg checks.
    Hook {
        #[command(subcommand)]
        command: Hook,
    },
    /// Evaluate pinned labeled cases and measure uncached pipeline latency.
    Eval(Eval),
    /// Fit empirical precision/coverage policy on fixed calibration cases.
    Calibrate(Calibrate),
    /// Locate original lines answering a query, with presence and uncertainty gates.
    Find(Selection),
    /// Rank candidate lines by Choice marginals or separate relevance Scores.
    Pick(Pick),
    /// Select source-backed date, money, version, address, host or ticker spans.
    Extract(Extract),
    /// Plan requests and count tokens, with no inference.
    Tokens(Classify),
    /// Inspect endpoint health, limits and tokenizer readiness.
    Doctor {
        #[arg(long)]
        prefetch: bool,
        #[arg(long)]
        json: bool,
    },
    /// List models, or report an unsupported endpoint.
    Models,
    /// Inspect, create, edit, trust and lint reusable rubrics.
    Presets {
        #[command(subcommand)]
        command: Presets,
    },
    /// Inspect and manage the local raw-response cache without inference.
    Cache {
        #[command(subcommand)]
        command: Cache,
    },
    /// Generate shell completions from this grammar.
    Completions { shell: clap_complete::Shell },
    /// Generate the complete roff manual, or one command's page.
    Man {
        /// Optional command path, such as "pick" or "presets trust".
        command: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum HookKind {
    PreCommit,
    CommitMsg,
}
impl HookKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::PreCommit => "pre-commit",
            Self::CommitMsg => "commit-msg",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Hook {
    /// Install both hooks, preserving existing files and core.hooksPath.
    Install {
        /// Explicit existing hooks directory; defaults to Git's effective path.
        #[arg(long)]
        directory: Option<PathBuf>,
        /// Select hooks; omission installs both.
        #[arg(value_enum)]
        hooks: Vec<HookKind>,
    },
    /// Run a hook check. Git supplies the message file to commit-msg.
    Run {
        #[arg(value_enum)]
        kind: HookKind,
        message: Option<PathBuf>,
        /// Evaluate an existing patch file instead of the staged Git diff.
        #[arg(long)]
        diff: Option<PathBuf>,
        /// Emit the exact joint request/counts without credentials or inference.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum EvalSplit {
    Test,
    Calibrate,
    Captured,
}

#[derive(Clone, Debug, Args)]
#[command(group(clap::ArgGroup::new("eval_input").args(["dataset", "labels"]).multiple(false)))]
pub struct Eval {
    /// Evaluate mean decisions from jointly encoded named preset variants.
    #[arg(long)]
    pub ensemble: bool,
    /// Minimum ensemble vote agreement (default 1.0 or preset policy).
    #[arg(long, requires = "ensemble", allow_hyphen_values = true)]
    pub min_agreement: Option<f64>,
    /// Labeled JSONL input; requires --preset and --dataset-revision.
    pub dataset: Option<PathBuf>,
    /// Explicit labeled JSONL input, equivalent to the positional dataset.
    #[arg(long, conflicts_with = "suite")]
    pub labels: Option<PathBuf>,
    /// Suite directory/manifest or name under --suites-dir; repeat for several suites.
    #[arg(long, conflicts_with = "eval_input")]
    pub suite: Vec<String>,
    #[arg(long, default_value = "evals/suites")]
    pub suites_dir: PathBuf,
    /// Trusted preset; also overrides a suite's preset for rubric comparisons.
    #[arg(short = 'p', long)]
    pub preset: Option<String>,
    #[arg(skip)]
    pub preset_override: Option<String>,
    #[arg(long, requires = "eval_input")]
    pub dataset_revision: Option<String>,
    #[arg(long, requires = "eval_input")]
    pub split_file: Option<PathBuf>,
    /// Fixed partition; defaults to test, or calibrate for sweeps.
    #[arg(long, value_enum)]
    pub partition: Option<EvalSplit>,
    /// Explicitly create missing fixed split files; existing assignments are validated.
    #[arg(long)]
    pub init_splits: bool,
    /// Write private JSON/Markdown reports and a publication manifest under DIR.
    #[arg(long)]
    pub report: Option<PathBuf>,
    /// Human-ratified baseline file; evaluate held-out absolute/regression gates.
    #[arg(long, conflicts_with_all = ["compare", "sweep_pack", "sweep_combine", "sweep_focus"])]
    pub baseline: Option<PathBuf>,
    /// Two report/profile/preset targets, or standalone/ensemble, over identical cases.
    #[arg(long, num_args = 2, conflicts_with_all = ["sweep_pack", "sweep_combine", "sweep_focus"])]
    pub compare: Vec<String>,
    /// Calibration-only packing candidates, separated by commas.
    #[arg(long, value_delimiter = ',', conflicts_with = "pack")]
    pub sweep_pack: Vec<loupe_core::pack::Mode>,
    /// Calibration-only combine candidates; implies explicit text splitting.
    #[arg(long, value_delimiter = ',', conflicts_with_all = ["combine", "focus", "truncate", "truncate_field"])]
    pub sweep_combine: Vec<loupe_core::combine::Mode>,
    /// Calibration-only selected-evidence counts; zero requests split/combine without focus.
    #[arg(long, value_delimiter = ',', conflicts_with_all = ["focus", "split_text", "truncate", "truncate_field"])]
    pub sweep_focus: Vec<usize>,
    #[arg(long)]
    pub json: bool,
    /// Emit provisional diagnostics when fresh full model identity is unavailable.
    #[arg(long)]
    pub allow_unverified: bool,
    /// Uncached passes over every selected packing group before latency measurement.
    #[arg(long, default_value = "1")]
    pub warmup: usize,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub no_cache: bool,
    #[arg(long, conflicts_with = "no_cache")]
    pub refresh: bool,
    #[arg(long)]
    pub strict: bool,
    #[arg(long)]
    pub budget: Option<u64>,
    #[arg(long)]
    pub deadline: Option<String>,
    #[arg(long)]
    pub pack: Option<loupe_core::pack::Mode>,
    /// Window question ID family; defaults to records or the calibrated preset's family.
    #[arg(long)]
    pub question_id_source: Option<loupe_core::pack::SourceKind>,
    #[arg(long="split", conflicts_with_all=["focus","truncate","truncate_field"])]
    pub split_text: bool,
    #[arg(long)]
    pub combine: Option<loupe_core::combine::Mode>,
    #[arg(long)]
    pub overlap: Option<usize>,
    #[arg(long)]
    pub max_chunks: Option<std::num::NonZeroUsize>,
    #[arg(long, conflicts_with_all=["split_text","combine","truncate","truncate_field"])]
    pub focus: Option<std::num::NonZeroUsize>,
    #[arg(long, conflicts_with_all=["split_text","focus","truncate_field"])]
    pub truncate: Option<loupe_core::truncate::Mode>,
    #[arg(long, conflicts_with_all=["split_text","focus","truncate"])]
    pub truncate_field: Option<String>,
    #[arg(long)]
    pub arg: Vec<String>,
    #[arg(short = 't', long, allow_hyphen_values = true)]
    pub threshold: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub margin: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub min_confidence: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub min_gap: Option<f64>,
}

#[derive(Debug, Args)]
#[command(group(clap::ArgGroup::new("calibration_goal").args(["target", "max_uncertain"]).required(true).multiple(true)))]
pub struct Calibrate {
    #[command(flatten)]
    pub evaluation: Eval,
    /// Select one field from a mixed rubric; a single-field preset needs no selector.
    #[arg(long)]
    pub field: Option<String>,
    /// Empirical precision=RATE or precision:LABEL=RATE; repeat for several labels.
    #[arg(long)]
    pub target: Vec<loupe_core::calibration::Target>,
    /// Maximum uncertain fraction, expressed as a probability or percentage.
    #[arg(long, value_parser = parse_fraction)]
    pub max_uncertain: Option<f64>,
    /// Minimum accepted/predicted support for every fitted precision target.
    #[arg(long, default_value = "1")]
    pub min_support: std::num::NonZeroUsize,
    /// Atomically write the fitted policy with complete current identity.
    #[arg(long)]
    pub write: bool,
}

fn parse_fraction(value: &str) -> Result<f64, String> {
    let (value, divisor) = value
        .strip_suffix('%')
        .map_or((value, 1.0), |value| (value, 100.0));
    let value = value
        .parse::<f64>()
        .map_err(|_| "expected a probability or percentage".to_owned())?
        / divisor;
    loupe_core::policy::Probability::new(value)
        .map(|value| value.get())
        .map_err(|error| error.to_string())
}

#[derive(Debug, Subcommand)]
pub enum Cache {
    Stats {
        #[arg(long)]
        json: bool,
    },
    Prune {
        #[arg(long = "older", default_value = "30d")]
        older_than: String,
    },
    Clear,
}

#[derive(Debug, Subcommand)]
pub enum Presets {
    List {
        #[arg(long)]
        json: bool,
    },
    Show {
        name: String,
    },
    New {
        name: String,
        #[arg(long, value_enum, default_value = "noul")]
        r#type: PresetType,
        #[arg(long)]
        project: bool,
    },
    Edit {
        name: String,
    },
    Trust {
        #[arg(conflicts_with = "file")]
        name: Option<String>,
        #[arg(short = 'f', long)]
        file: Option<PathBuf>,
    },
    Lint {
        name: Option<String>,
        #[arg(long)]
        live: bool,
        #[arg(long)]
        arg: Vec<String>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PresetType {
    Noul,
    Choice,
    Score,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Input {
    Lines,
    Whole,
    Null,
    Paragraphs,
    Records,
    Jsonl,
    Csv,
    Tsv,
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum Output {
    Plain,
    Annotate,
    Tsv,
    Json,
    Csv,
    Parquet,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Uncertain {
    Keep,
    Drop,
    Fail,
    Stderr,
    Fallback,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum FallbackError {
    Uncertain,
    Fail,
}

#[derive(Clone, Debug, Args)]
pub struct Classify {
    #[command(flatten)]
    pub common: Common,
    /// Retain the head/tail, or remove the middle while retaining both ends.
    #[arg(long, conflicts_with_all = ["split", "truncate_field", "focus"])]
    pub truncate: Option<loupe_core::truncate::Mode>,
}

#[derive(Debug, Args)]
pub struct Selection {
    #[command(flatten)]
    pub common: Common,
    /// Retain up to N candidates, ranked within the final Choice population.
    #[arg(short = 'n', long, default_value = "1")]
    pub take: std::num::NonZeroUsize,
    /// Stop before submitting a round beyond this bound.
    #[arg(long, default_value = "64")]
    pub max_rounds: std::num::NonZeroUsize,
}

#[derive(Debug, Args)]
pub struct Pick {
    #[command(flatten)]
    pub selection: Selection,
    /// Evaluate each candidate in its own Score/gate request.
    #[arg(long, conflicts_with = "max_rounds")]
    pub independent: bool,
}

#[derive(Debug, Args)]
pub struct Extract {
    #[command(flatten)]
    pub selection: Selection,
    /// Parse dates, money, semver, emails, hostnames or tickers from original text.
    #[arg(long)]
    pub candidates: loupe_core::extract::Kind,
    /// Normalize the selected source span in code, retaining verbatim provenance.
    #[arg(long)]
    pub normalize: bool,
    /// Explicit RFC 3339 clock for relative/yearless dates.
    #[arg(long, requires = "normalize")]
    pub now: Option<String>,
    /// IANA timezone for relative and offset-free dates.
    #[arg(long, requires = "normalize")]
    pub tz: Option<String>,
    /// ISO 4217 code to resolve ambiguous money symbols.
    #[arg(long, requires = "normalize")]
    pub currency: Option<String>,
    #[arg(long, requires = "normalize")]
    pub decimal_separator: Option<loupe_core::normalize::Separator>,
    #[arg(long, requires = "normalize")]
    pub date_order: Option<loupe_core::normalize::DateOrder>,
}
#[derive(Debug, Args)]
pub struct On {
    #[command(flatten)]
    pub options: Classify,
    /// Start an action group for yes, no, uncertain, or a declared Choice label.
    #[arg(long)]
    pub when: Vec<String>,
    /// Execute one shell command for each matching unit, without action retries.
    #[arg(long = "exec")]
    pub exec: Vec<String>,
    /// Append matching original unit bytes to this file.
    #[arg(long)]
    pub append: Vec<PathBuf>,
    #[arg(long, default_value = "30s")]
    pub exec_timeout: String,
    #[arg(long)]
    pub dedupe_window: Option<String>,
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// Maximum live dedupe claims, default 65536.
    #[arg(long)]
    pub max_fired: Option<std::num::NonZeroUsize>,
    #[arg(skip)]
    pub ordered: Vec<loupe_core::trigger::Declaration>,
}
#[derive(Debug, Args)]
pub struct Dedupe {
    #[command(flatten)]
    pub options: Classify,
    #[arg(long, default_value = "first")]
    pub keep: loupe_core::dedupe::Keep,
    /// Refuse a finite catalog requiring more than N pair requests.
    #[arg(long, conflicts_with_all = ["window", "window_time"])]
    pub max_pairs: Option<u64>,
    /// Retain emitted representatives from the previous N physical input units.
    #[arg(long, conflicts_with = "window_time")]
    pub window: Option<std::num::NonZeroUsize>,
    /// Retain representatives for a monotonic duration, such as 10m.
    #[arg(long, conflicts_with = "window")]
    pub window_time: Option<String>,
    /// Maximum live representatives in a time window; overflow fails explicitly.
    #[arg(long, requires = "window_time")]
    pub index_capacity: Option<std::num::NonZeroUsize>,
    /// Maximum serialized bytes of retained streaming representatives.
    #[arg(long)]
    pub max_index_bytes: Option<std::num::NonZeroUsize>,
    /// Maximum LSH candidates per arriving streaming unit.
    #[arg(long)]
    pub max_candidates: Option<std::num::NonZeroUsize>,
}

#[derive(Debug, Args)]
pub struct Join {
    /// Jointly evaluate the pair rubric's named variants.
    #[arg(long)]
    pub ensemble: bool,
    /// Minimum pair-ensemble agreement, default 1.0 or preset policy.
    #[arg(long, requires = "ensemble", allow_hyphen_values = true)]
    pub min_agreement: Option<f64>,
    /// Structured left input, or - for a streaming stdin source.
    pub left: String,
    /// Finite right input, fully loaded before the left side is read.
    pub right: String,
    /// Comma-separated scalar field paths for local candidate blocking.
    #[arg(long)]
    pub on: String,
    #[arg(long, default_value = "20")]
    pub max_candidates: std::num::NonZeroUsize,
    #[arg(long, default_value = "hybrid")]
    pub blocking: loupe_core::blocking::Mode,
    #[arg(short = 'I', long, value_enum, default_value = "jsonl")]
    pub input_format: Input,
    /// Emit candidate diagnostics without model inference.
    #[arg(long)]
    pub dry_run: bool,
    #[arg(short = 'p', long, conflicts_with = "question_file")]
    pub preset: Option<String>,
    #[arg(short = 'f', long, conflicts_with = "preset")]
    pub question_file: Option<PathBuf>,
    #[arg(long)]
    pub arg: Vec<String>,
    #[arg(long)]
    pub context: Option<PathBuf>,
    #[arg(long, default_value = "1:1")]
    pub cardinality: loupe_core::matching::Cardinality,
    /// Emit unmatched left/right records; bare --unmatched means both.
    #[arg(long,value_enum,num_args=0..=1,default_missing_value="both")]
    pub unmatched: Option<Unmatched>,
    #[arg(short = 'O', long, value_enum, conflicts_with = "json")]
    pub output_format: Option<Output>,
    #[arg(long)]
    pub json: bool,
    #[arg(short = 't', long, allow_hyphen_values = true)]
    pub threshold: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub margin: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub min_confidence: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub min_gap: Option<f64>,
    #[arg(long, value_enum)]
    pub on_uncertain: Option<Uncertain>,
    /// Resolve uncertain pair gates/scores using a trusted shell command.
    #[arg(long)]
    pub fallback: Option<String>,
    #[arg(long)]
    pub fallback_timeout: Option<String>,
    #[arg(long, value_enum)]
    pub on_fallback_error: Option<FallbackError>,
    #[arg(long)]
    pub why: bool,
    #[arg(long)]
    pub strict: bool,
    #[arg(long)]
    pub no_cache: bool,
    #[arg(long, conflicts_with = "no_cache")]
    pub refresh: bool,
    #[arg(long)]
    pub summary: bool,
    #[arg(long)]
    pub budget: Option<u64>,
    #[arg(long)]
    pub deadline: Option<String>,
    #[arg(long)]
    pub keep_going: bool,
    #[arg(long, value_enum, requires = "keep_going")]
    pub on_error: Option<OnError>,
    #[arg(long, conflicts_with = "resume")]
    pub failed: Option<PathBuf>,
    #[arg(long)]
    pub resume: Option<PathBuf>,
}
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Unmatched {
    Left,
    Right,
    Both,
}
impl std::ops::Deref for Classify {
    type Target = Common;
    fn deref(&self) -> &Common {
        &self.common
    }
}
#[derive(Debug, Args)]
pub struct Route {
    #[command(flatten)]
    pub common: Common,
    /// Append to DIR/<sanitized-label>; process destinations override their label.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Feed one persistent shell command per declared label: LABEL=COMMAND.
    #[arg(long)]
    pub to: Vec<String>,
    /// Start fresh through a recoverable reset of owned files and route state.
    #[arg(long = "truncate", id = "route_truncate", conflicts_with = "resume")]
    pub truncate: bool,
    /// Maximum wait for each process write and EOF shutdown (default 30s).
    #[arg(long)]
    pub to_timeout: Option<String>,
}
#[derive(Clone, Debug, Args)]
pub struct Common {
    /// Positional question followed by files; with -p/-f, all operands are files.
    pub operands: Vec<String>,
    #[arg(short = 'p', long, conflicts_with = "question_file")]
    pub preset: Option<String>,
    #[arg(short = 'f', long, conflicts_with = "preset")]
    pub question_file: Option<PathBuf>,
    #[arg(short = 'c', long, conflicts_with = "criteria_file")]
    pub criteria: Option<String>,
    #[arg(long, conflicts_with = "criteria")]
    pub criteria_file: Option<PathBuf>,
    #[arg(long)]
    pub context: Option<PathBuf>,
    #[arg(long)]
    pub arg: Vec<String>,
    #[arg(short = 't', long, allow_hyphen_values = true)]
    pub threshold: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub margin: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub min_confidence: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    pub min_gap: Option<f64>,
    /// Jointly evaluate named preset variants and average their distributions.
    #[arg(long)]
    pub ensemble: bool,
    /// Minimum vote agreement for ensemble certainty (default 1.0).
    #[arg(long, requires = "ensemble", allow_hyphen_values = true)]
    pub min_agreement: Option<f64>,
    #[arg(long, value_enum)]
    pub on_uncertain: Option<Uncertain>,
    /// Resolve uncertain primary decisions with this trusted shell command.
    #[arg(long)]
    pub fallback: Option<String>,
    /// Maximum duration per fallback command (default 30s).
    #[arg(long)]
    pub fallback_timeout: Option<String>,
    #[arg(long, value_enum)]
    pub on_fallback_error: Option<FallbackError>,
    #[arg(long)]
    pub uncertain: Option<PathBuf>,
    #[arg(short = 'I', long, value_enum)]
    pub input_format: Option<Input>,
    #[arg(short = 'O', long, value_enum, conflicts_with = "json")]
    pub output_format: Option<Output>,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub keep_blank: bool,
    /// Show declared companion signal probabilities and their separate policies.
    #[arg(long)]
    pub why: bool,
    #[arg(long)]
    pub records: Option<String>,
    #[arg(long)]
    pub field: Option<String>,
    #[arg(long)]
    pub select: Option<String>,
    #[arg(long)]
    pub with_context: Option<usize>,
    #[arg(long = "files")]
    pub files_mode: bool,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub strict: bool,
    #[arg(long)]
    pub budget: Option<u64>,
    #[arg(long)]
    pub deadline: Option<String>,
    /// Pack independent units, whole input, or joint windows (window:N).
    #[arg(long)]
    pub pack: Option<loupe_core::pack::Mode>,
    /// Explicitly split oversized text under the selected combine semantics.
    #[arg(long)]
    pub split: bool,
    #[arg(long)]
    pub combine: Option<loupe_core::combine::Mode>,
    #[arg(long)]
    pub overlap: Option<usize>,
    #[arg(long)]
    pub max_chunks: Option<std::num::NonZeroUsize>,
    /// Score chunks, then re-ask over at most K selected evidence chunks.
    #[arg(long, conflicts_with_all = ["split", "combine", "truncate_field"])]
    pub focus: Option<std::num::NonZeroUsize>,
    /// Shorten only this string field in a JSONL/CSV/TSV record, retaining its head.
    #[arg(long, conflicts_with = "split")]
    pub truncate_field: Option<String>,
    #[arg(long)]
    pub no_cache: bool,
    #[arg(long, conflicts_with = "no_cache")]
    pub refresh: bool,
    #[arg(long)]
    pub summary: bool,
    /// Review uncertainty plus a uniform audit sample using /dev/tty, or defer.
    #[arg(long,value_enum,num_args=0..=1,require_equals=true,default_missing_value="interactive")]
    pub review: Option<ReviewMode>,
    /// Durable queue path (default PROJECT/.loupe/review.jsonl).
    #[arg(long, requires = "review")]
    pub review_queue: Option<PathBuf>,
    /// Captured labels directory (default PROJECT/.loupe/labels).
    #[arg(long, requires = "review")]
    pub review_labels: Option<PathBuf>,
    /// Uniform review probability, default 2%; percentage or fraction.
    #[arg(long, requires = "review")]
    pub audit_rate: Option<String>,
    /// Continue after unit preflight or inference failures (default action: drop).
    #[arg(long)]
    pub keep_going: bool,
    /// Handle failed units independently of uncertain answers.
    #[arg(long, value_enum, requires = "keep_going")]
    pub on_error: Option<OnError>,
    /// Persist admissions, exact plans, results and output progress in a new JSONL journal.
    #[arg(long, conflicts_with = "resume")]
    pub failed: Option<PathBuf>,
    /// Validate original input and resume a run journal (or route's manifest).
    #[arg(long)]
    pub resume: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, ValueEnum, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewMode {
    Interactive,
    Defer,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum, Default, PartialEq, Eq)]
pub enum OnError {
    #[default]
    Drop,
    Keep,
    Fail,
}

pub(crate) fn on_declarations(
    matches: &clap::ArgMatches,
) -> loupe_core::error::Result<Vec<loupe_core::trigger::Declaration>> {
    use loupe_core::error::{Error, ErrorKind};
    use loupe_core::trigger::{Declaration, Target};
    enum Event {
        When(String),
        Action(Target),
    }
    let mut events = Vec::new();
    if let (Some(indices), Some(values)) = (
        matches.indices_of("when"),
        matches.get_many::<String>("when"),
    ) {
        events.extend(
            indices
                .zip(values)
                .map(|(index, value)| (index, Event::When(value.clone()))),
        );
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of("exec"),
        matches.get_many::<String>("exec"),
    ) {
        events.extend(
            indices
                .zip(values)
                .map(|(index, value)| (index, Event::Action(Target::Exec(value.clone())))),
        );
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of("append"),
        matches.get_many::<PathBuf>("append"),
    ) {
        events.extend(
            indices
                .zip(values)
                .map(|(index, value)| (index, Event::Action(Target::Append(value.clone())))),
        );
    }
    events.sort_by_key(|(index, _)| *index);
    let mut when = None;
    let mut group_has_action = false;
    let mut declarations = Vec::new();
    for (_, event) in events {
        match event {
            Event::When(value) => {
                if when.is_some() && !group_has_action {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "each --when group needs an --exec or --append",
                    ));
                }
                when = Some(value);
                group_has_action = false;
            }
            Event::Action(target) => {
                let when = when.as_ref().ok_or_else(|| {
                    Error::new(
                        ErrorKind::Usage,
                        "--exec/--append require a preceding --when",
                    )
                })?;
                declarations.push(Declaration {
                    when: when.clone(),
                    target,
                });
                group_has_action = true;
            }
        }
    }
    if !group_has_action {
        return Err(Error::new(
            ErrorKind::Usage,
            "on requires at least one complete --when action group",
        ));
    }
    Ok(declarations)
}
