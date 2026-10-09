use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use clap::CommandFactory;
use futures::{StreamExt, TryStreamExt};
use indexmap::IndexMap;
use serde_json::{Value, json};
use tokio::time::Instant;

use loupe_core::backend::{Backend, Evaluation, with_deadline};
use loupe_core::budget::Budget;
use loupe_core::cache_session::Mode as CacheMode;
use loupe_core::combine::Mode as CombineMode;
use loupe_core::config::{Config, Settings, xdg_path};
use loupe_core::encoder::{self, ClefEncoder, Counts};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::pack::{self, Mode as PackMode, SourceKind, Window};
use loupe_core::policy::{
    Assessment, Destination, Filter, Policy, PolicyFlag, Probability, QuestionKind, Selected,
    UncertainAction,
};
use loupe_core::preset_store::{Preset, Store};
use loupe_core::presets;
use loupe_core::records::{Format, Options as InputOptions, Source};
use loupe_core::scheduler::Batcher;
use loupe_core::split;
use loupe_core::templates;
use loupe_core::truncate;
use loupe_core::wire::{Question, Request, Response, Usage};

use crate::args::{Classify, Cli, Command, Input, OnError, Output, Uncertain};
use crate::run::failure::{WorkError, WorkResult};
use crate::run::focus_jobs::{FocusFinal, FocusSettings, evaluate_focus_final};

mod calibrated;
#[cfg(test)]
mod compatibility;
mod dedupe;
mod eval;
mod explain;
mod failure;
mod fallback;
mod features;
mod focus_jobs;
mod hooks;
mod matching;
#[cfg(feature = "mcp")]
mod mcp;
mod recovery;
mod review;
mod selection;
mod trigger;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Is,
    Keep,
    Drop,
    Tag,
    Route,
    Rate,
    Sort,
    Count,
    Ask,
    Featurize,
    Explain,
    Tokens,
    Pick,
    Dedupe,
    On,
}

struct Prepared {
    questions: IndexMap<String, Question>,
    files: Vec<String>,
    policy: Policy,
    uncertain: UncertainAction,
    fallback: bool,
    document: Value,
    signals: loupe_core::signals::Set,
    ensembles: loupe_core::ensemble::Set,
    min_agreement: Probability,
}

impl Prepared {
    fn primary(&self) -> IndexMap<String, Question> {
        self.ensembles
            .primary(&self.signals.primary(&self.questions))
    }
}

#[derive(Clone, Copy)]
struct Why<'a> {
    signals: &'a loupe_core::signals::Set,
    primary: &'a str,
}

impl Why<'_> {
    fn reports<'a>(
        &self,
        completed: &'a Completed,
    ) -> Result<IndexMap<String, loupe_core::signals::Report<'a>>> {
        self.signals.reports(
            &completed.request,
            &completed
                .evaluation
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing signal response"))?
                .response,
            self.primary,
            &completed.answer_id,
        )
    }
}

fn signal_text(reports: &IndexMap<String, loupe_core::signals::Report<'_>>) -> String {
    reports
        .iter()
        .map(|(id, report)| {
            format!(
                "{id} {:.4} {}",
                report.probability.get(),
                if report.assessment.uncertain {
                    "uncertain".to_owned()
                } else {
                    selected(&report.assessment)
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

struct Completed {
    unit: Unit,
    request: Arc<Request>,
    counts: Arc<Counts>,
    evaluation: Option<Arc<Evaluation>>,
    answer_id: String,
    packed_indices: Option<Vec<u64>>,
    split_plan: Option<Value>,
    view: Option<Value>,
    focus_plan: Option<Value>,
    fallback: Option<Arc<loupe_core::fallback::Execution>>,
    ensemble: Option<Value>,
    review: Option<Value>,
}

enum Job {
    Saved(Box<Completed>),
    Units(Vec<Result<Unit>>),
    Window {
        units: Vec<Unit>,
        packed: Box<pack::Packed>,
    },
    Split(Box<SplitJob>),
    View(Box<Completed>),
    FocusPlan(Box<Completed>),
    FocusFinal(Box<FocusFinal>),
}

struct SplitJob {
    unit: Unit,
    request: Arc<Request>,
    counts: Arc<Counts>,
    chunks: Vec<split::Chunk>,
    modes: IndexMap<String, CombineMode>,
    packed_indices: Option<Vec<u64>>,
    path: String,
}

struct SplitSettings {
    options: split::Options,
    modes: IndexMap<String, CombineMode>,
    path: String,
}

struct TruncateSettings {
    mode: truncate::Mode,
    path: String,
}

struct Execution {
    backend: Option<Backend>,
    encoder: Option<Arc<ClefEncoder>>,
    questions: IndexMap<String, Question>,
    model: String,
    limits: loupe_core::config::Limits,
    budget: Budget,
    strict: bool,
    deadline: Option<Instant>,
    use_batch: bool,
    concurrency: usize,
    pack: PackMode,
    contextual: bool,
    source: SourceKind,
    split: Option<SplitSettings>,
    truncate: Option<TruncateSettings>,
    focus: Option<FocusSettings>,
    keep_going: bool,
    recovery: Option<recovery::Registry>,
}
impl Execution {
    fn record_plan(
        &self,
        indices: Vec<u64>,
        request: &Request,
        counts: &Counts,
        extra: Value,
    ) -> Result<()> {
        if let Some(registry) = &self.recovery {
            registry.plan(indices,json!({"request":request,"question_order":request.questions.keys().collect::<Vec<_>>(),"counts":counts,"provenance":extra}))?;
        }
        Ok(())
    }
}

pub async fn execute(cli: Cli) -> Result<u8> {
    let started = Instant::now();
    if let Command::Completions { shell } = &cli.command {
        let mut rendered = Vec::new();
        clap_complete::generate(*shell, &mut Cli::command(), "loupe", &mut rendered);
        io::stdout()
            .lock()
            .write_all(&rendered)
            .map_err(output_error)?;
        return Ok(0);
    }
    if let Command::Man { command } = &cli.command {
        crate::manual::render(command, &mut io::stdout().lock())?;
        return Ok(0);
    }
    let env: HashMap<_, _> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .collect();
    let cwd =
        std::env::current_dir().map_err(|error| Error::io("reading working directory", error))?;
    let store = Store::new(&cwd, &env)?;
    if let Command::Review { file, labels_dir } = &cli.command {
        return review::consume(
            file,
            labels_dir
                .as_deref()
                .unwrap_or(&store.project_root.join(".loupe/labels")),
            cli.quiet,
        )
        .await;
    }
    let path = cli
        .config
        .clone()
        .unwrap_or(xdg_path(&env, "XDG_CONFIG_HOME", ".config")?.join("config.toml"));
    let config = Config::load(&path, cli.config.is_some())?;
    let settings = Settings {
        base_url: cli.url.clone(),
        path: cli.path.clone(),
        api_format: cli.api_format,
        model: cli.model.clone(),
        concurrency: cli.concurrency,
        ..Settings::default()
    };
    let mut profile = config.resolve(cli.profile.as_deref(), &settings, &env)?;
    if let Command::Mcp { deadline, budget } = &cli.command {
        #[cfg(feature = "mcp")]
        return crate::run::mcp::execute(store, profile, &config.mcp, duration(deadline)?, *budget)
            .await;
        #[cfg(not(feature = "mcp"))]
        {
            let _ = (deadline, budget);
            return Err(Error::new(
                ErrorKind::Usage,
                "MCP requires a build with --features mcp",
            ));
        }
    }
    if let Command::Hook { command } = &cli.command {
        return crate::run::hooks::execute(&cli, command, &store, profile, &config.hooks, started)
            .await;
    }
    if let Command::Join(options) = &cli.command {
        return crate::run::matching::execute(
            &cli,
            options,
            &store,
            profile,
            &config.fallback,
            started,
        )
        .await;
    }
    if let Command::Dedupe(options) = &cli.command {
        return crate::run::dedupe::execute(
            &cli,
            options,
            &store,
            profile,
            &config.fallback,
            started,
        )
        .await;
    }
    if let Command::On(options) = &cli.command {
        return crate::run::trigger::execute(
            &cli,
            options,
            &store,
            profile,
            &config.fallback,
            started,
        )
        .await;
    }
    if let Command::Presets { command } = &cli.command {
        return crate::preset_commands::execute(command, &store, &profile).await;
    }
    if let Command::Cache { command } = &cli.command {
        return crate::cache_commands::execute(command, &profile.cache_dir);
    }
    if let Command::Eval(options) = &cli.command {
        return crate::run::eval::execute(&cli, options, &store, profile, &config).await;
    }
    if let Command::Calibrate(options) = &cli.command {
        return crate::run::eval::calibrate::execute(&cli, options, &store, profile).await;
    }
    if let Command::Find(options) = &cli.command {
        return crate::run::selection::execute(
            &cli,
            options,
            &store,
            profile,
            &config.fallback,
            started,
            crate::run::selection::Mode::Find,
        )
        .await;
    }
    if let Command::Pick(options) = &cli.command {
        let mode = if options.independent {
            crate::run::selection::Mode::Independent
        } else {
            crate::run::selection::Mode::Pick
        };
        return crate::run::selection::execute(
            &cli,
            &options.selection,
            &store,
            profile,
            &config.fallback,
            started,
            mode,
        )
        .await;
    }
    if let Command::Extract(options) = &cli.command {
        return crate::run::selection::execute(
            &cli,
            &options.selection,
            &store,
            profile,
            &config.fallback,
            started,
            crate::run::selection::Mode::Extract(options.candidates),
        )
        .await;
    }
    match &cli.command {
        Command::Models => {
            write_json(
                &mut io::stdout().lock(),
                &Backend::new(&profile).await?.models().await?,
            )?;
            return Ok(0);
        }
        Command::Doctor {
            prefetch,
            json: as_json,
        } => {
            if *prefetch && profile.encoder == "clef-v1" {
                encoder::prefetch(&profile).await?;
            }
            let encoder = encoder::load_profile(&profile);
            let tokenizer = match &encoder {
                Ok(Some(encoder)) => json!({"ready":true,"identity":encoder.identity}),
                Ok(None) => json!({"ready":true,"exact":false}),
                Err(error) => json!({"ready":false,"error":error.to_string()}),
            };
            let discovery = Backend::new(&profile)
                .await?
                .discover(&profile.limits)
                .await?;
            let healthy = discovery
                .health
                .as_ref()
                .and_then(|v| v.get("status"))
                .and_then(Value::as_str)
                .is_some_and(|status| status == "ready" || status == "ok");
            let cache_eligible = discovery.identity.as_ref().is_some_and(|identity| {
                encoder.as_ref().is_ok_and(|encoder| {
                    encoder
                        .as_ref()
                        .is_none_or(|encoder| encoder.identity == identity.encoder_identity)
                })
            });
            let cache_status = if cache_eligible {
                "eligible, disk availability checked when classifying"
            } else if discovery.identity.is_none() {
                "disabled, authoritative full identity unavailable"
            } else {
                "disabled, local encoder unavailable or identity differs"
            };
            if *as_json {
                write_json(
                    &mut io::stdout().lock(),
                    &json!({"profile":profile.name,"endpoint":format!("{}{}",profile.base_url, profile.path),"discovery":discovery,"tokenizer":tokenizer,"cache_eligible":cache_eligible}),
                )?;
            } else {
                let mut stdout = io::stdout().lock();
                writeln!(
                    stdout,
                    "profile: {}\nendpoint: {}{}\nhealth: {}\ntokenizer: {}\ncache: {}\nlimits: {}",
                    profile.name,
                    profile.base_url,
                    profile.path,
                    if healthy {
                        "ready"
                    } else {
                        "unavailable/not ready"
                    },
                    tokenizer,
                    cache_status,
                    serde_json::to_string(&discovery.limits).map_err(json_error)?
                )
                .map_err(output_error)?;
                for unsupported in &discovery.unsupported {
                    writeln!(stdout, "unsupported: {unsupported}").map_err(output_error)?;
                }
                for unavailable in &discovery.unavailable {
                    writeln!(stdout, "unavailable: {unavailable}").map_err(output_error)?;
                }
                writeln!(
                    stdout,
                    "batch: {}",
                    serde_json::to_string(&discovery.batch).map_err(json_error)?
                )
                .map_err(output_error)?;
            }
            return Ok(if healthy && encoder.is_ok() { 0 } else { 69 });
        }
        _ => {}
    }
    let route_options = match &cli.command {
        Command::Route(args) => Some(Classify {
            common: args.common.clone(),
            truncate: None,
        }),
        _ => None,
    };
    let (kind, options, reverse) = match &cli.command {
        Command::Is(options) => (Kind::Is, options, false),
        Command::Keep(options) => (Kind::Keep, options, false),
        Command::Drop(options) => (Kind::Drop, options, false),
        Command::Tag(options) => (Kind::Tag, options, false),
        Command::Route(_) => (
            Kind::Route,
            route_options
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::Usage, "route options are missing"))?,
            false,
        ),
        Command::Rate(options) => (Kind::Rate, options, false),
        Command::Sort { options, reverse } => (Kind::Sort, options, *reverse),
        Command::Count(options) => (Kind::Count, options, false),
        Command::Ask(options) => (Kind::Ask, options, false),
        Command::Featurize(options) => (Kind::Featurize, options, false),
        Command::Explain(options) => (Kind::Explain, options, false),
        Command::Tokens(options) => (Kind::Tokens, options, false),
        _ => {
            return Err(Error::new(
                ErrorKind::Usage,
                "command does not evaluate input",
            ));
        }
    };
    if options.keep_going
        && (matches!(kind, Kind::Is | Kind::Ask | Kind::Tokens | Kind::Explain) || options.dry_run)
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "--keep-going applies to commands emitting unit decisions, not is/ask/tokens or --dry-run",
        ));
    }
    let prepared = prepare(kind, options, &store, &profile.limits, &profile.model)?;
    if let Some(pin) = loupe_core::calibration_pin::read(&prepared.document)? {
        pin.check_ensemble(options.ensemble, prepared.min_agreement)?;
    }
    let effective_options = calibrated::options(options, &prepared.document)?;
    let options = &effective_options;
    let fallback = fallback::Plan::resolve(kind, options, &prepared, &config.fallback)?;
    let factor_set = if kind == Kind::Explain {
        Some(loupe_core::factors::Set::from_document(&prepared.document)?)
    } else {
        None
    };
    let feature_schema = if kind == Kind::Featurize {
        Some(loupe_core::features::Schema::new(prepared.primary())?)
    } else {
        None
    };
    let output = if options.json {
        Output::Json
    } else {
        options.output_format.unwrap_or(if kind == Kind::Featurize {
            Output::Csv
        } else {
            Output::Plain
        })
    };
    if kind == Kind::Featurize && !matches!(output, Output::Csv | Output::Json | Output::Parquet)
        || kind != Kind::Featurize && matches!(output, Output::Csv | Output::Parquet)
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "featurize accepts -O csv|json|parquet; CSV/Parquet feature output applies only to featurize",
        ));
    }
    if output == Output::Parquet {
        #[cfg(not(feature = "parquet"))]
        return Err(Error::new(
            ErrorKind::Usage,
            "Parquet requires a build with --features parquet",
        ));
        #[cfg(feature = "parquet")]
        if options.failed.is_some() || options.resume.is_some() {
            return Err(Error::new(
                ErrorKind::Usage,
                "Parquet stdout is a complete file and cannot use --failed/--resume; journal CSV/JSON before conversion",
            ));
        }
    }
    let why = if options.why {
        Some(Why {
            signals: &prepared.signals,
            primary: prepared
                .questions
                .first()
                .map(|(id, _)| id.as_str())
                .ok_or_else(|| Error::new(ErrorKind::Usage, "primary question missing"))?,
        })
    } else {
        None
    };
    let general_resume = options
        .resume
        .as_deref()
        .filter(|path| kind != Kind::Route || !crate::route::Plan::manifest_resume(path));
    if (options.failed.is_some() || general_resume.is_some())
        && (kind == Kind::Tokens || options.dry_run)
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "--failed/--resume apply to inference runs, not token plans or dry runs",
        ));
    }
    let route_plan = match &cli.command {
        Command::Route(args) => Some(crate::route::Plan::new(
            args,
            &prepared.primary(),
            prepared.uncertain,
        )?),
        _ => None,
    };
    let route_description = route_plan.as_ref().map(crate::route::Plan::description);
    let declared_pack = prepared
        .document
        .get("policy")
        .and_then(|policy| policy.get("pack"))
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| {
                    Error::new(ErrorKind::Usage, "preset packing mode must be a string")
                })?
                .parse::<PackMode>()
        })
        .transpose()?;
    let requested_pack = options.pack.or(declared_pack).unwrap_or_default();
    let pack = if requested_pack == PackMode::Auto {
        declared_pack
            .filter(|pack| *pack != PackMode::Auto)
            .unwrap_or(PackMode::Unit)
    } else {
        requested_pack
    };
    if matches!(pack, PackMode::Window(_)) && matches!(kind, Kind::Is | Kind::Ask) {
        return Err(Error::new(
            ErrorKind::Usage,
            "is/ask require whole-input requests; window packing applies to per-unit commands",
        ));
    }
    let mut reader_options = InputOptions::from_document(
        &prepared.document,
        if matches!(kind, Kind::Is | Kind::Ask | Kind::Tokens | Kind::Explain) {
            Format::Whole
        } else {
            if kind == Kind::Featurize {
                Format::Jsonl
            } else {
                Format::Lines
            }
        },
    )?;
    let input = match options.input_format {
        Some(Input::Lines) => Format::Lines,
        Some(Input::Whole) => Format::Whole,
        Some(Input::Null) => Format::Null,
        Some(Input::Paragraphs) => Format::Paragraphs,
        Some(Input::Records) => Format::Records,
        Some(Input::Jsonl) => Format::Jsonl,
        Some(Input::Csv) => Format::Csv,
        Some(Input::Tsv) => Format::Tsv,
        None => reader_options.format,
    };
    let input = if options.files_mode {
        if options
            .input_format
            .is_some_and(|format| !matches!(format, Input::Whole))
            || prepared.files.is_empty()
            || prepared.files.iter().any(|file| file == "-")
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "--files requires named file operands and is incompatible with other input formats",
            ));
        }
        Format::Files
    } else {
        input
    };
    if matches!(kind, Kind::Is | Kind::Ask)
        && (!matches!(input, Format::Whole | Format::Files) || prepared.files.len() > 1)
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "is/ask require one whole input stream",
        ));
    }
    if pack == PackMode::Whole && !matches!(input, Format::Whole | Format::Files) {
        return Err(Error::new(
            ErrorKind::Usage,
            "--pack whole requires -I whole",
        ));
    }
    if options.with_context.is_some() && input != Format::Lines {
        return Err(Error::new(
            ErrorKind::Usage,
            "--with-context applies only to lines input",
        ));
    }
    reader_options.format = input;
    reader_options.keep_blank |= options.keep_blank;
    if options.records.is_some() {
        reader_options.records = options.records.clone();
    }
    if options.field.is_some() {
        reader_options.field = options.field.clone();
    }
    if options.select.is_some() {
        reader_options.select = options
            .select
            .as_ref()
            .map(|columns| {
                columns
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .collect()
            })
            .unwrap_or_default();
    }
    if let Some(context) = options.with_context {
        reader_options.with_context = context;
    }
    reader_options.validate()?;
    let contextual = reader_options.with_context != 0;
    let structured = matches!(input, Format::Jsonl | Format::Csv | Format::Tsv);
    if options.truncate.is_some() && structured || options.truncate_field.is_some() && !structured {
        return Err(Error::new(
            ErrorKind::Usage,
            "--truncate applies to text; --truncate-field applies to JSONL/CSV/TSV records",
        ));
    }
    let text_path = if contextual {
        "/unit".to_owned()
    } else if input == Format::Files {
        "/content".to_owned()
    } else {
        String::new()
    };
    let focus_settings = if let Some(k) = options.focus {
        if structured {
            return Err(Error::new(
                ErrorKind::Usage,
                "focus applies to text; structured JSONL/CSV/TSV records cannot split",
            ));
        }
        Some(FocusSettings {
            k,
            options: split::Options {
                overlap: options.overlap.unwrap_or(256),
                max_chunks: options.max_chunks.map(|value| value.get()).unwrap_or(64),
            },
            path: text_path.clone(),
        })
    } else {
        None
    };
    let truncate_settings = if let Some(path) = &options.truncate_field {
        let parts = loupe_core::records::field_path(path)?;
        let path = parts
            .iter()
            .map(|part| format!("/{}", part.replace('~', "~0").replace('/', "~1")))
            .collect::<String>();
        Some(TruncateSettings {
            mode: truncate::Mode::Head,
            path,
        })
    } else {
        options.truncate.map(|mode| TruncateSettings {
            mode,
            path: text_path.clone(),
        })
    };
    let split_enabled = options.split
        || truncate_settings.is_none()
            && focus_settings.is_none()
            && prepared
                .document
                .get("policy")
                .and_then(|policy| policy.get("splittable"))
                == Some(&Value::Bool(true));
    if !split_enabled
        && focus_settings.is_none()
        && (options.combine.is_some() || options.overlap.is_some() || options.max_chunks.is_some())
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "combine requires splitting; overlap/max-chunks require splitting or focus",
        ));
    }
    if split_enabled && structured {
        return Err(Error::new(
            ErrorKind::Usage,
            "structured JSONL/CSV/TSV records cannot split",
        ));
    }
    let split_settings = if split_enabled {
        let mut modes = loupe_core::combine::resolve(
            &prepared.ensembles.primary(&prepared.questions),
            prepared
                .document
                .get("policy")
                .and_then(|policy| policy.get("combine")),
            options.combine,
        )?;
        prepared.ensembles.extend_combine(&mut modes)?;
        Some(SplitSettings {
            options: split::Options {
                overlap: options.overlap.unwrap_or(256),
                max_chunks: options.max_chunks.map(|value| value.get()).unwrap_or(64),
            },
            modes,
            path: text_path.clone(),
        })
    } else {
        None
    };
    let encoder = encoder::load_profile(&profile)?.map(Arc::new);
    if (split_settings.is_some() || truncate_settings.is_some() || focus_settings.is_some())
        && encoder.is_none()
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "token-aligned splitting/truncation/focus requires a prepared clef-v1 tokenizer",
        ));
    }
    if options.budget.is_some() && encoder.is_none() {
        return Err(Error::new(
            ErrorKind::Usage,
            "hard --budget requires verified clef-v1 input-token counts",
        ));
    }
    let deadline = options
        .deadline
        .as_deref()
        .map(duration)
        .transpose()?
        .or(if kind == Kind::Is {
            Some(Duration::from_secs(60))
        } else {
            None
        })
        .map(|duration| {
            started
                .checked_add(duration)
                .ok_or_else(|| Error::new(ErrorKind::Usage, "deadline is outside the clock range"))
        })
        .transpose()?;
    let planning = kind == Kind::Tokens || options.dry_run;
    let budget = Budget::new(options.budget);
    let mut summary = crate::summary::Summary::new(
        options.summary,
        planning,
        budget.clone(),
        profile.pricing.clone(),
    );
    let mut use_batch = false;
    let mut batch_size = 1;
    let mut model_identity = None;
    let backend = if planning {
        None
    } else {
        let (backend, discovery) = with_deadline(deadline, async {
            let backend = Backend::new(&profile)
                .await?
                .with_request_trace(cli.verbose > 1);
            let discovery = backend.discover(&profile.limits).await?;
            Ok((backend, discovery))
        })
        .await?;
        profile.limits = discovery.limits.clone();
        model_identity = discovery.full_model_identity.clone();
        use_batch = discovery.batch.supported
            && pack == PackMode::Unit
            && !matches!(kind, Kind::Is | Kind::Ask);
        if use_batch {
            batch_size = discovery.batch.max_records.min(profile.concurrency);
        }
        if prepared
            .document
            .get("policy")
            .and_then(|policy| policy.get("model_identity"))
            .is_some()
        {
            let preset = Preset {
                name: "effective".to_owned(),
                document: prepared.document.clone(),
                origin: loupe_core::preset_store::Origin::Builtin,
                trusted: true,
                content_hash: String::new(),
            };
            preset.check_policy_identity(discovery.full_model_identity.as_deref())?;
        }
        if let Some(pin) = loupe_core::calibration_pin::read(&prepared.document)? {
            if !matches!(kind, Kind::Ask | Kind::Tokens | Kind::Featurize) {
                let question = prepared.questions.get(&pin.field).ok_or_else(|| {
                    Error::new(
                        ErrorKind::Usage,
                        "calibrated field is absent from this request",
                    )
                })?;
                pin.check_policy(&prepared.document, prepared.policy, question.kind())?;
            }
            let mut packing = json!({"declared":format!("{pack:?}"),"combine":split_settings.as_ref().map(|settings| &settings.modes).cloned().unwrap_or_default(),"split":split_settings.is_some(),"focus":options.focus,"truncate":truncate_settings.as_ref().map(|settings| settings.mode),"path":truncate_settings.as_ref().map_or(text_path.as_str(), |settings| settings.path.as_str())});
            if split_settings.is_some() || focus_settings.is_some() {
                packing["overlap"] = json!(options.overlap.unwrap_or(256));
                packing["max_chunks"] =
                    json!(options.max_chunks.map(|value| value.get()).unwrap_or(64));
            }
            if matches!(pack, PackMode::Window(_)) {
                let source = match input {
                    Format::Lines => SourceKind::Lines,
                    Format::Paragraphs
                    | Format::Records
                    | Format::Csv
                    | Format::Tsv
                    | Format::Jsonl => SourceKind::Records,
                    Format::Files => SourceKind::Files,
                    _ => SourceKind::Units,
                };
                packing["question_id_source"] = json!(format!("{source:?}"));
            }
            let view = loupe_core::calibration_pin::view(
                &packing,
                &serde_json::to_value(&profile.limits).map_err(json_error)?,
            )?;
            pin.check(
                discovery.full_model_identity.as_deref(),
                encoder
                    .as_ref()
                    .map_or("estimate-v1", |encoder| encoder.identity.as_str()),
                discovery
                    .identity
                    .as_ref()
                    .map(|identity| identity.encoder_identity.as_str()),
                &view,
            )?;
        }
        let mode = if options.no_cache {
            CacheMode::Disabled
        } else if options.refresh {
            CacheMode::Refresh
        } else {
            CacheMode::Normal
        };
        let (backend, warning) = with_deadline(
            deadline,
            backend.with_cache(&discovery, encoder.clone(), &profile.cache_dir, mode),
        )
        .await?;
        if let Some(warning) = warning
            && !cli.quiet
        {
            eprintln!("loupe: {warning}");
        }
        Some(backend)
    };
    if split_settings.is_some() || truncate_settings.is_some() || focus_settings.is_some() {
        use_batch = false;
        batch_size = 1;
    }
    let source = match input {
        Format::Lines => SourceKind::Lines,
        Format::Paragraphs | Format::Records | Format::Csv | Format::Tsv | Format::Jsonl => {
            SourceKind::Records
        }
        Format::Files => SourceKind::Files,
        _ => SourceKind::Units,
    };
    if let PackMode::Window(size) = pack {
        batch_size = Window {
            encoder: encoder.as_deref(),
            limits: &profile.limits,
            questions: &prepared.questions,
            model: &profile.model,
            contextual,
            source,
        }
        .max_units(size)?;
    }
    let mut route_identity = route_plan.as_ref().map(|_| json!({
        "version":1,"document":prepared.document,"questions":prepared.questions,"files":prepared.files,"reader":reader_options,
        "policy":{"threshold":prepared.policy.threshold.get(),"margin":prepared.policy.margin,"min_confidence":prepared.policy.min_confidence.get(),"min_gap":prepared.policy.min_gap,"uncertain":format!("{:?}",prepared.uncertain)},
        "endpoint":format!("{}{}",profile.base_url,profile.path),"model":profile.model,"model_identity":model_identity,
        "encoder":encoder.as_ref().map(|encoder| &encoder.identity),"pack":format!("{pack:?}"),"limits":profile.limits,
        "output":format!("{output:?}"),"split":split_settings.as_ref().map(|settings| json!({"options":{"overlap":settings.options.overlap,"max_chunks":settings.options.max_chunks},"modes":settings.modes,"path":settings.path})),
        "truncate":truncate_settings.as_ref().map(|settings| json!({"mode":settings.mode,"path":settings.path})),
        "focus":focus_settings.as_ref().map(|settings| json!({"k":settings.k,"overlap":settings.options.overlap,"max_chunks":settings.options.max_chunks,"path":settings.path})),
        "route":route_description,"errors":{"keep_going":options.keep_going,"action":format!("{:?}",options.on_error.unwrap_or_default())}
    }));
    if !options.keep_going
        && let Some(identity) = route_identity.as_mut().and_then(Value::as_object_mut)
    {
        identity.remove("errors");
    }
    if let Some(fallback) = &fallback
        && let Some(identity) = &mut route_identity
    {
        identity["fallback"] = serde_json::to_value(fallback).map_err(json_error)?;
    }
    if options.ensemble
        && let Some(identity) = &mut route_identity
    {
        identity["ensemble"] = json!({"enabled":true,"min_agreement":prepared.min_agreement});
    }
    let mut normalized_route = route_identity.clone();
    if let Some(route) = normalized_route
        .as_mut()
        .and_then(|identity| identity.get_mut("route"))
        .and_then(Value::as_object_mut)
    {
        route.remove("resume");
        route.remove("reset");
    }
    let mut recovery_identity = json!({
        "version":1,"command":format!("{kind:?}"),"reverse":reverse,"strict":options.strict,"cwd":cwd,"document":prepared.document,
        "questions":prepared.questions.iter().collect::<Vec<_>>(),"files":prepared.files,"reader":reader_options,
        "policy":{"threshold":prepared.policy.threshold.get(),"margin":prepared.policy.margin,"min_confidence":prepared.policy.min_confidence.get(),"min_gap":prepared.policy.min_gap,"uncertain":format!("{:?}",prepared.uncertain),"capture":options.uncertain},
        "endpoint":format!("{}{}",profile.base_url,profile.path),"model":profile.model,"model_identity":model_identity,
        "encoder":encoder.as_ref().map(|encoder|&encoder.identity),"pack":format!("{pack:?}"),"limits":profile.limits,"output":format!("{output:?}"),
        "split":split_settings.as_ref().map(|settings|json!({"overlap":settings.options.overlap,"max_chunks":settings.options.max_chunks,"modes":settings.modes,"path":settings.path})),
        "truncate":truncate_settings.as_ref().map(|settings|json!({"mode":settings.mode,"path":settings.path})),
        "focus":focus_settings.as_ref().map(|settings|json!({"k":settings.k,"overlap":settings.options.overlap,"max_chunks":settings.options.max_chunks,"path":settings.path})),
        "route":normalized_route,"errors":{"keep_going":options.keep_going,"action":format!("{:?}",options.on_error.unwrap_or_default())}
    });
    if let Some(fallback) = &fallback {
        recovery_identity["fallback"] = serde_json::to_value(fallback).map_err(json_error)?;
    }
    if options.review.is_some() {
        recovery_identity["review"] = json!({"mode":options.review,"queue":options.review_queue,"labels":options.review_labels,"audit_rate":review::rate(options)?});
    }
    if options.ensemble {
        recovery_identity["ensemble"] =
            json!({"enabled":true,"min_agreement":prepared.min_agreement});
    }
    if options.why {
        recovery_identity["why"] = json!(true);
        if let Some(identity) = route_identity.as_mut() {
            identity["why"] = json!(true);
        }
        if let Some(identity) = recovery_identity
            .get_mut("route")
            .filter(|value| !value.is_null())
        {
            identity["why"] = json!(true);
        }
    }
    let session = options
        .failed
        .as_deref()
        .or(general_resume)
        .map(|path| {
            if let Command::Route(args) = &cli.command
                && let Some(out) = &args.out
            {
                let parent = path
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                if let (Ok(parent), Ok(out)) = (parent.canonicalize(), out.canonicalize())
                    && parent.starts_with(out)
                {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "general recovery journal must be outside the route output directory",
                    ));
                }
            }
            recovery::Registry::open(
                path,
                general_resume.is_some(),
                recovery_identity,
                &prepared.files,
                options.uncertain.as_deref(),
            )
        })
        .transpose()?;
    let recovery = session.as_ref().map(|session| session.0.clone());
    let mut router = if planning {
        None
    } else {
        route_plan
            .map(|plan| {
                let mut identity = route_identity
                    .clone()
                    .ok_or_else(|| Error::new(ErrorKind::Usage, "route identity is missing"))?;
                if let Some(route) = identity.get_mut("route").and_then(Value::as_object_mut) {
                    route.remove("resume");
                    route.remove("reset");
                }
                let bytes = serde_json::to_vec(&identity).map_err(json_error)?;
                plan.start(
                    blake3::hash(&bytes).to_hex().as_str(),
                    &prepared.files,
                    deadline,
                    cli.quiet,
                )
            })
            .transpose()?
    };
    let replay = router.as_ref().map(crate::route::Router::replay);
    let mut uncertain_file = if planning || kind == Kind::Route {
        None
    } else {
        options
            .uncertain
            .as_ref()
            .map(|path| {
                reject_input_output_alias(path, &prepared.files)?;
                if let Some(recovery) = &recovery {
                    recovery.reject_output_alias(path)?;
                }
                OpenOptions::new()
                    .read(true)
                    .create(true)
                    .append(true)
                    .open(path)
                    .map_err(|error| Error::io("opening uncertainty output", error))
            })
            .transpose()?
    };
    let mut uncertain_header = None;
    let selected_feature_id = reader_options.field.is_some();
    let review_files = prepared.files.clone();
    let rx = read_units(
        prepared.files,
        reader_options,
        profile.concurrency,
        recovery.clone(),
        (kind == Kind::Featurize).then_some((selected_feature_id, source)),
    )?;
    let batcher = Batcher::new(rx, batch_size, Duration::from_millis(10))?;
    let batches = futures::stream::unfold(batcher, |mut batcher| async move {
        batcher.next().await.map(|batch| (batch, batcher))
    });
    let batches = batches.map(move |batch| match &replay {
        Some(replay) => replay.filter(batch),
        None => batch,
    });
    let execution = Arc::new(Execution {
        backend,
        encoder: encoder.clone(),
        questions: prepared.questions.clone(),
        model: profile.model.clone(),
        limits: profile.limits.clone(),
        budget: budget.clone(),
        strict: options.strict,
        deadline,
        use_batch,
        concurrency: profile.concurrency,
        pack,
        contextual,
        source,
        split: split_settings,
        truncate: truncate_settings,
        focus: focus_settings,
        keep_going: options.keep_going,
        recovery: recovery.clone(),
    });
    let jobs = batches
        .map(|batch| {
            let execution = execution.clone();
            async move { recovery::jobs(batch, &execution).await }
        })
        .buffered(if execution.focus.is_some() {
            profile.concurrency.div_ceil(batch_size)
        } else {
            1
        })
        .flat_map(futures::stream::iter);
    let futures = jobs
        .map(|job| evaluate_job(job, execution.clone()))
        .buffered(
            if matches!(pack, PackMode::Window(_))
                || execution.split.is_some()
                || execution.truncate.is_some()
                || execution.focus.is_some()
            {
                profile.concurrency
            } else {
                profile.concurrency.div_ceil(batch_size)
            },
        )
        .flat_map(futures::stream::iter);
    futures::pin_mut!(futures);
    let interrupt = interrupt_signal();
    tokio::pin!(interrupt);
    let mut stdout = io::stdout().lock();
    let mut review = review::Plan::new(
        options,
        &store.project_root,
        &review_files,
        encoder
            .as_ref()
            .map_or("estimate-v1", |encoder| encoder.identity.as_str()),
        recovery.as_ref(),
        cli.quiet,
    )?;
    let feature_output = feature_schema
        .as_ref()
        .map(|schema| {
            features::Output::new(
                schema,
                output,
                selected_feature_id,
                source,
                planning,
                options.ensemble.then_some(loupe_core::features::Ensemble {
                    set: &prepared.ensembles,
                    document: &prepared.document,
                    min_agreement: prepared.min_agreement,
                }),
            )
        })
        .transpose()?;
    if !planning
        && general_resume.is_none()
        && output == Output::Csv
        && let Some(features) = &feature_output
    {
        features.schema.csv_header(&mut stdout)?;
    }
    let mut emitted = if kind == Kind::Route {
        0
    } else {
        recovery
            .as_ref()
            .map(recovery::Registry::original_emitted)
            .unwrap_or(0)
    };
    let mut is_code = 1;
    let mut histogram = BTreeMap::<String, u64>::new();
    let mut sorted = Vec::new();
    let mut sorted_errors = Vec::new();
    let mut error_count = 0u64;
    let mut emitted_header: Option<Vec<u8>> = None;
    let result = async { loop {
        let item = tokio::select! {
            item = futures.next() => item,
            signal = &mut interrupt => { if let Some(recovery)=&recovery {recovery.stop();} stdout.flush().map_err(output_error)?; return signal; }
            _ = async { if let Some(deadline) = deadline { tokio::time::sleep_until(deadline).await; } else { std::future::pending::<()>().await; } } => { if let Some(recovery)=&recovery {recovery.stop();} stdout.flush().map_err(output_error)?; return Err(recorded(Error::new(ErrorKind::Unavailable, "overall deadline expired"),&recovery,None)); }
        };
        let Some(item) = item else { break };
        let mut completed = match item {
            Ok(completed) => completed,
            Err(failure) => {
                if failure.unit.is_some() {
                    summary.error()?;
                }
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    stdout.flush().map_err(output_error)?;
                    return Err(Error::new(
                        ErrorKind::Unavailable,
                        "overall deadline expired",
                    ));
                }
                if !options.keep_going
                    || !failure.recoverable()
                    || matches!(options.on_error, Some(OnError::Fail))
                {
                    stdout.flush().map_err(output_error)?;
                    return Err(failure.error);
                }
                let unit = failure.unit.ok_or_else(|| {
                    Error::new(ErrorKind::Unavailable, "unit failure context missing")
                })?;
                let keep = matches!(options.on_error, Some(OnError::Keep));
                let index = unit.index;
                let mut acknowledge = kind != Kind::Count;
                if !cli.quiet {
                    eprintln!(
                        "loupe: unit {}: {} (--on-error {})",
                        unit.index,
                        failure.error,
                        if keep { "keep" } else { "drop" }
                    );
                }
                if kind == Kind::Route {
                    let mut payload = Vec::new();
                    if keep {
                        failure::emit(&mut payload, &unit, &failure.error, output, kind)?;
                    }
                    let router = router
                        .as_mut()
                        .ok_or_else(|| Error::new(ErrorKind::Usage, "route writer is missing"))?;
                    let delivered = tokio::select! {
                        result = router.deliver_error(&unit, keep.then_some(payload), input, output) => result?,
                        signal = &mut interrupt => return signal,
                    };
                    emitted += u64::from(delivered);
                } else if kind == Kind::Count {
                    error_count = error_count
                        .checked_add(1)
                        .ok_or_else(|| Error::new(ErrorKind::Input, "error count overflow"))?;
                } else if keep {
                    if let Some(features) = &feature_output {
                        features.failure(&mut stdout, &unit, &failure.error)?;
                        emitted += 1;
                        stdout.flush().map_err(output_error)?;
                    } else if kind == Kind::Sort {
                        sorted_errors.push((unit, failure.error));
                        acknowledge = false;
                    } else {
                        emit_header(&mut stdout, &unit, output, kind, &mut emitted_header)?;
                        failure::emit(&mut stdout, &unit, &failure.error, output, kind)?;
                        emitted += 1;
                        stdout.flush().map_err(output_error)?;
                    }
                }
                if acknowledge && let Some(recovery) = &recovery {
                    recovery.handled(index, keep)?;
                }
                continue;
            }
        };
        summary.unit()?;
        if planning {
            let mut plan = json!({"index":completed.unit.index,"request":completed.request.as_ref(),"counts":completed.counts.as_ref(),"encoder":encoder.as_ref().map(|encoder| &encoder.identity)});
            if options.ensemble {
                plan["ensemble"] = json!({"method":"equal_mean_probabilities","independent":false,"min_agreement":prepared.min_agreement});
            }
            if let Some(schema) = &feature_schema {
                plan["feature_columns"] = json!(schema.columns());
            }
            if let Some(indices) = &completed.packed_indices {
                plan["indices"] = json!(indices);
            }
            if let Some(split) = &completed.split_plan {
                plan["split"] = split.clone();
            }
            if let Some(view) = &completed.view {
                plan["truncate"] = view.clone();
            }
            if let Some(focus) = &completed.focus_plan {
                plan["focus"] = focus.clone();
            }
            if let Some(route) = &route_description {
                plan["route"] = route.clone();
            }
            write_json(&mut stdout, &plan)?;
            emitted += 1;
            continue;
        }
        let evaluation = completed
            .evaluation
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing evaluation"))?;
        if let Some(warning) = &evaluation.warning
            && !cli.quiet
        {
            eprintln!("loupe: unit {}: {warning}", completed.unit.index);
        }
        if kind == Kind::Ask {
            write_json(&mut stdout, &evaluation.response)?;
            stdout.flush().map_err(output_error)?;
            if let Some(recovery) = &recovery {
                recovery.handled(completed.unit.index, true)?;
            }
            emitted += 1;
            continue;
        }
        if let Some(factors) = &factor_set {
            crate::run::explain::emit(
                &mut stdout,
                &completed,
                factors,
                prepared
                    .questions
                    .first()
                    .map(|(id, _)| id.as_str())
                    .ok_or_else(|| Error::new(ErrorKind::Usage, "primary question missing"))?,
                output,
                why,
                options.ensemble.then_some(loupe_core::factors::Ensemble {
                    set: &prepared.ensembles,
                    min_agreement: prepared.min_agreement,
                }),
            )
            .map_err(|error| recorded(error, &recovery, Some(completed.unit.index)))?;
            stdout.flush().map_err(output_error)?;
            if let Some(recovery) = &recovery {
                recovery.handled(completed.unit.index, true)?;
            }
            emitted += 1;
            continue;
        }
        if let Some(features) = &feature_output {
            features
                .emit(&mut stdout, &completed, why, cli.quiet)
                .map_err(|error| recorded(error, &recovery, Some(completed.unit.index)))?;
            stdout.flush().map_err(output_error)?;
            if let Some(recovery) = &recovery {
                recovery.handled(completed.unit.index, true)?;
            }
            emitted += 1;
            continue;
        }
        let answer = evaluation
            .response
            .validate(&completed.request)
            .map_err(|error| recorded(error, &recovery, Some(completed.unit.index)))?
            .shift_remove(&completed.answer_id)
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "response has no answer"))?;
        let mut assessment = answer
            .assessment(&prepared.policy)
            .map_err(|error| recorded(error, &recovery, Some(completed.unit.index)))?;
        let primary = prepared
            .questions
            .first()
            .map(|(id, _)| id.as_str())
            .ok_or_else(|| Error::new(ErrorKind::Usage, "primary question missing"))?;
        let ensemble = if prepared.ensembles.has_primary(primary) {
            let report = prepared.ensembles.report(
                primary,
                &completed.answer_id,
                &completed.request,
                &evaluation.response,
                prepared.policy,
                prepared.min_agreement,
            )?;
            assessment = report.assessment.clone();
            completed.ensemble = Some(serde_json::to_value(report).map_err(json_error)?);
            Some(loupe_core::fallback::Ensemble {
                set: &prepared.ensembles,
                primary,
                min_agreement: prepared.min_agreement,
            })
        } else {
            None
        };
        if let Some(plan) = &fallback {
            let input = loupe_core::fallback::Input {
                unit: &completed.unit,
                request: &completed.request,
                response: &evaluation.response,
                field: &completed.answer_id,
                policy: prepared.policy,
            };
            let result = tokio::select! {
                result = plan.execute(&input, &mut assessment, completed.fallback.clone(), deadline, &mut summary, ensemble.as_ref()) => result,
                signal = &mut interrupt => { stdout.flush().map_err(output_error)?; return signal; },
            }.map_err(|error| recorded(error, &recovery, Some(completed.unit.index)))?;
            if completed.fallback.is_none()
                && let Some(result) = &result
                && let Some(recovery) = &recovery
            {
                recovery.fallback(
                    completed.unit.index,
                    serde_json::to_value(result.as_ref()).map_err(json_error)?,
                )?;
            }
            completed.fallback = result;
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                stdout.flush().map_err(output_error)?;
                return Err(recorded(
                    Error::new(
                        ErrorKind::Unavailable,
                        "overall deadline expired during fallback",
                    ),
                    &recovery,
                    Some(completed.unit.index),
                ));
            }
            plan.require_success(completed.fallback.as_deref())
                .map_err(|error| recorded(error, &recovery, Some(completed.unit.index)))?;
        }
        if let Some(review)=&mut review {
            let decision=tokio::select!{result=with_deadline(deadline,review.assess(&completed,&mut assessment))=>result,signal=&mut interrupt=>return signal}.map_err(|error|recorded(error,&recovery,Some(completed.unit.index)))?;
            if completed.review.is_none() && let Some(recovery)=&recovery {recovery.review(completed.unit.index,decision.clone())?;}
            completed.review=Some(decision);
        }
        if let Some(why) = why
            && !cli.quiet
        {
            eprintln!(
                "loupe: unit {} {}{} [{}]",
                completed.unit.index,
                selected(&assessment),
                if assessment.uncertain {
                    " uncertain"
                } else {
                    ""
                },
                signal_text(&why.reports(&completed)?)
            );
        }
        if kind == Kind::Route {
            if assessment.uncertain {
                summary.uncertain()?;
            }
            let mut payload = Vec::new();
            emit(&mut payload, &completed, &assessment, output, kind, why)?;
            let router = router
                .as_mut()
                .ok_or_else(|| Error::new(ErrorKind::Usage, "route writer is missing"))?;
            let delivered = tokio::select! {
                result = router.deliver(&completed.unit, &assessment, prepared.uncertain, payload, input, output) => result?,
                signal = &mut interrupt => return signal,
            };
            emitted += u64::from(delivered);
            if let Some(recovery) = &recovery {
                recovery.handled(completed.unit.index, delivered)?;
            }
            continue;
        }
        if assessment.uncertain {
            summary.uncertain()?;
            if let Some(file) = &mut uncertain_file {
                append_header(file, &completed.unit, input, &mut uncertain_header)?;
                file.write_all(&completed.unit.raw)
                    .map_err(|error| Error::io("writing uncertainty output", error))?;
                file.flush()
                    .map_err(|error| Error::io("flushing uncertainty output", error))?;
            }
        }
        if kind == Kind::Is {
            is_code = assessment.test_exit_code()?;
            if output != Output::Plain {
                emit(&mut stdout, &completed, &assessment, output, kind, why)?;
            }
            if cli.verbose != 0 && !cli.quiet {
                if review::human(completed.review.as_ref())?.is_some() {
                    eprintln!("loupe: source=review confidence=unavailable uncertain=false");
                } else if matches!(
                    completed.fallback.as_deref(),
                    Some(loupe_core::fallback::Execution::Resolved { .. })
                ) {
                    eprintln!("loupe: source=fallback confidence=unavailable uncertain=false");
                } else {
                    eprintln!(
                        "loupe: confidence={} uncertain={}",
                        assessment.confidence.get(),
                        assessment.uncertain
                    );
                }
            }
            stdout.flush().map_err(output_error)?;
            continue;
        }
        if kind == Kind::Count {
            let label = if assessment.uncertain {
                "uncertain".to_owned()
            } else {
                selected(&assessment)
            };
            *histogram.entry(label).or_default() += 1;
            continue;
        }
        let destination = match kind {
            Kind::Keep => assessment.filter(Filter::Keep, prepared.uncertain),
            Kind::Drop => assessment.filter(Filter::Drop, prepared.uncertain),
            _ => assessment.destination(prepared.uncertain),
        }
        .map_err(|error| recorded(error, &recovery, Some(completed.unit.index)))?;
        let index = completed.unit.index;
        match destination {
            Destination::Omit => {}
            Destination::Stderr => {
                io::stderr()
                    .lock()
                    .write_all(&completed.unit.raw)
                    .map_err(|error| Error::io("writing uncertain unit to stderr", error))?;
            }
            Destination::Stdout => {
                if kind == Kind::Sort {
                    sorted.push((completed, assessment));
                } else {
                    emit_header(
                        &mut stdout,
                        &completed.unit,
                        output,
                        kind,
                        &mut emitted_header,
                    )?;
                    emit(&mut stdout, &completed, &assessment, output, kind, why)?;
                    emitted += 1;
                    stdout.flush().map_err(output_error)?;
                }
            }
        }
        if (kind != Kind::Sort || destination != Destination::Stdout)
            && let Some(recovery) = &recovery
        {
            recovery.handled(index, destination == Destination::Stdout)?;
        }
    }
    if let Some(router) = &mut router {
        let skipped = tokio::select! { result = router.finish() => result?, signal = &mut interrupt => return signal };
        emitted = emitted
            .checked_add(skipped)
            .ok_or_else(|| Error::new(ErrorKind::Input, "route emitted count overflow"))?;
    }
    if let Some((_, code)) = recovery.as_ref().and_then(recovery::Registry::finished) {
        return Ok(code);
    }
    if kind == Kind::Count {
        if output == Output::Json {
            let mut result = json!({"counts":histogram});
            if error_count != 0 {
                result["errors"] = json!(error_count);
            }
            write_json(&mut stdout, &result)?;
        } else {
            for (label, count) in &histogram {
                writeln!(stdout, "{count}\t{label}").map_err(output_error)?;
            }
            if error_count != 0 {
                writeln!(stdout, "{error_count}\t@errors").map_err(output_error)?;
            }
        }
        emitted = u64::from(!histogram.is_empty() || error_count != 0);
    }
    if kind == Kind::Sort {
        sorted.sort_by(|(_, a), (_, b)| {
            let score = |assessment: &Assessment| match assessment.selected {
                Selected::Score(value) => value,
                _ => 0.0,
            };
            let order = score(a).total_cmp(&score(b));
            if reverse { order.reverse() } else { order }
        });
        for (completed, assessment) in sorted {
            emit_header(
                &mut stdout,
                &completed.unit,
                output,
                kind,
                &mut emitted_header,
            )?;
            emit(&mut stdout, &completed, &assessment, output, kind, why)?;
            emitted += 1;
            stdout.flush().map_err(output_error)?;
            if let Some(recovery) = &recovery {
                recovery.handled(completed.unit.index, true)?;
            }
        }
        for (unit, error) in sorted_errors {
            emit_header(&mut stdout, &unit, output, kind, &mut emitted_header)?;
            failure::emit(&mut stdout, &unit, &error, output, kind)?;
            emitted += 1;
            stdout.flush().map_err(output_error)?;
            if let Some(recovery) = &recovery {
                recovery.handled(unit.index, true)?;
            }
        }
    }
    stdout.flush().map_err(output_error)?;
    let code = if kind == Kind::Is && !planning {
        is_code
    } else {
        u8::from(emitted == 0)
    };
    if let Some(recovery) = &recovery {
        if let Some((_, code)) = recovery.finished() {
            return Ok(code);
        }
        recovery.finish(emitted, code)?;
    }
    Ok(code)
    }.await;
    let finished = feature_output
        .as_ref()
        .map(|output| output.finish(&mut stdout))
        .transpose();
    match (result, finished) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(code), Ok(_)) => Ok(code),
    }
}

fn recorded(error: Error, recovery: &Option<recovery::Registry>, index: Option<u64>) -> Error {
    if let Some(recovery) = recovery
        && let Err(journal_error) = recovery.failure(index, &error)
    {
        return journal_error;
    }
    error
}

fn interrupt_signal() -> impl std::future::Future<Output = Result<u8>> {
    #[cfg(unix)]
    {
        let terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate());
        let interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt());
        async move {
            let mut terminate =
                terminate.map_err(|error| Error::io("listening for SIGTERM", error))?;
            let mut interrupt =
                interrupt.map_err(|error| Error::io("listening for SIGINT", error))?;
            tokio::select! {
                _ = interrupt.recv() => Ok(130),
                _ = terminate.recv() => Ok(143),
            }
        }
    }
    #[cfg(not(unix))]
    {
        async {
            tokio::signal::ctrl_c()
                .await
                .map_err(|error| Error::io("listening for interruption", error))?;
            Ok(130)
        }
    }
}

async fn evaluate_records(
    records: Vec<Result<Unit>>,
    execution: Arc<Execution>,
) -> Vec<WorkResult<Completed>> {
    let mut planned = Vec::new();
    let mut output = Vec::new();
    for record in records {
        let unit = match record {
            Ok(unit) => unit,
            Err(error) => {
                output.push(Some(Err(error.into())));
                break;
            }
        };
        let index = unit.index;
        let prepare = || {
            let request = unit_request(&unit, &execution);
            execution.limits.validate_request(&request)?;
            let counts = pack::count(execution.encoder.as_deref(), &request)?;
            counts.preflight(&execution.limits)?;
            Ok::<_, Error>((request, counts))
        };
        match prepare() {
            Ok((request, counts)) => {
                let slot = output.len();
                output.push(None);
                planned.push((
                    slot,
                    Completed {
                        unit,
                        answer_id: request
                            .questions
                            .first()
                            .map(|(id, _)| id.clone())
                            .unwrap_or_default(),
                        request: Arc::new(request),
                        counts: Arc::new(counts),
                        evaluation: None,
                        packed_indices: None,
                        split_plan: None,
                        view: None,
                        focus_plan: None,
                        fallback: None,
                        ensemble: None,
                        review: None,
                    },
                ));
            }
            Err(error) => {
                output.push(Some(Err(WorkError::at(
                    Error::new(error.kind, format!("unit {index}: {error}")),
                    &unit,
                ))));
                if !execution.keep_going {
                    break;
                }
            }
        }
    }
    if let Some(backend) = &execution.backend
        && !planned.is_empty()
    {
        for (_, record) in &planned {
            if let Err(error) = execution.record_plan(
                vec![record.unit.index],
                &record.request,
                &record.counts,
                json!({"operation":"unit"}),
            ) {
                return vec![Err(error.into())];
            }
        }
        let result = if execution.use_batch {
            let requests = planned
                .iter()
                .map(|(_, record)| record.request.as_ref().clone())
                .collect::<Vec<_>>();
            let counts = planned
                .iter()
                .map(|(_, record)| record.counts.as_ref().clone())
                .collect::<Vec<_>>();
            backend
                .evaluate_batch(
                    &requests,
                    &counts,
                    &execution.budget,
                    execution.strict,
                    execution.deadline,
                )
                .await
        } else {
            Ok(futures::stream::iter(&planned)
                .map(|(_, record)| async {
                    backend
                        .evaluate(
                            &record.request,
                            &record.counts,
                            &execution.budget,
                            execution.strict,
                            execution.deadline,
                        )
                        .await
                })
                .buffered(execution.concurrency)
                .collect::<Vec<_>>()
                .await)
        };
        match result {
            Err(error) => {
                for (slot, record) in planned {
                    output[slot] = Some(Err(WorkError::at(
                        Error::new(error.kind, format!("unit {}: {error}", record.unit.index)),
                        &record.unit,
                    )));
                }
            }
            Ok(results) => {
                let mut results = results.into_iter();
                for (slot, mut record) in planned {
                    let index = record.unit.index;
                    let result = results.next().unwrap_or_else(|| {
                        Err(Error::new(
                            ErrorKind::Unavailable,
                            "batch response omitted unit",
                        ))
                    });
                    output[slot] = Some(match result {
                        Ok(evaluation) => {
                            record.evaluation = Some(Arc::new(evaluation));
                            Ok(record)
                        }
                        Err(error) => Err(WorkError::at(
                            Error::new(error.kind, format!("unit {index}: {error}")),
                            &record.unit,
                        )),
                    });
                }
            }
        }
    } else {
        for (slot, record) in planned {
            output[slot] = Some(Ok(record));
        }
    }
    output.into_iter().flatten().collect()
}

fn plan_jobs(records: Vec<Result<Unit>>, execution: &Execution) -> Vec<WorkResult<Job>> {
    let PackMode::Window(size) = execution.pack else {
        if execution.split.is_some() || execution.truncate.is_some() {
            return records
                .into_iter()
                .map(|record| {
                    let unit = record?;
                    let original = unit.clone();
                    let result = (|| {
                        validate_view_target(&unit, execution)?;
                        let request = unit_request(&unit, execution);
                        execution.limits.validate_request(&request)?;
                        let counts = pack::count(execution.encoder.as_deref(), &request)?;
                        match counts.preflight(&execution.limits) {
                            Ok(()) => Ok(Job::Units(vec![Ok(unit)])),
                            Err(error) if error.kind == ErrorKind::Input => oversize_job(
                                unit,
                                request,
                                counts,
                                view_path(execution)?,
                                execution
                                    .split
                                    .as_ref()
                                    .map(|settings| settings.modes.clone())
                                    .unwrap_or_default(),
                                None,
                                execution,
                            ),
                            Err(error) => Err(error),
                        }
                    })();
                    result.map_err(|error| WorkError::at(error, &original))
                })
                .collect();
        }
        return vec![Ok(Job::Units(records))];
    };
    let mut units = Vec::new();
    let mut jobs = Vec::new();
    for record in records {
        let record = record.map_err(WorkError::from).and_then(|unit| {
            validate_view_target(&unit, execution).map_err(|error| WorkError::at(error, &unit))?;
            Ok(unit)
        });
        match record {
            Ok(unit) => units.push(unit),
            Err(error) => {
                jobs.extend(plan_window(&units, size, execution));
                units.clear();
                let continue_run = execution.keep_going && error.recoverable();
                jobs.push(Err(error));
                if !continue_run {
                    return jobs;
                }
            }
        }
    }
    jobs.extend(plan_window(&units, size, execution));
    jobs
}

fn plan_window(
    units: &[Unit],
    size: std::num::NonZeroUsize,
    execution: &Execution,
) -> Vec<WorkResult<Job>> {
    let window = Window {
        encoder: execution.encoder.as_deref(),
        limits: &execution.limits,
        questions: &execution.questions,
        model: &execution.model,
        contextual: execution.contextual,
        source: execution.source,
    };
    let mut position = 0;
    let mut jobs = Vec::new();
    while position < units.len() {
        let Some(remaining) = units.get(position..) else {
            jobs.push(Err(Error::new(
                ErrorKind::Unavailable,
                "window cursor out of range",
            )
            .into()));
            return jobs;
        };
        let packed = match window.prefix(remaining, size) {
            Ok(packed) => packed,
            Err(error)
                if error.kind == ErrorKind::Input
                    && (execution.split.is_some() || execution.truncate.is_some()) =>
            {
                let recovery = || {
                    let unit = remaining
                        .first()
                        .ok_or_else(|| Error::new(ErrorKind::Input, "split window unit missing"))?;
                    let packed = window.single(unit)?;
                    let binding = packed.bindings.first().ok_or_else(|| {
                        Error::new(ErrorKind::Input, "split window binding missing")
                    })?;
                    let modes = binding
                        .questions
                        .iter()
                        .filter_map(|(base, bound)| {
                            execution
                                .split
                                .as_ref()
                                .and_then(|settings| settings.modes.get(base))
                                .map(|mode| (bound.clone(), *mode))
                        })
                        .collect::<IndexMap<_, _>>();
                    oversize_job(
                        unit.clone(),
                        packed.request,
                        packed.counts,
                        &format!("/units/{}{}", binding.source_id, view_path(execution)?),
                        modes,
                        Some(vec![unit.index]),
                        execution,
                    )
                };
                match recovery() {
                    Ok(job) => {
                        jobs.push(Ok(job));
                        position += 1;
                        continue;
                    }
                    Err(error) => {
                        let Some(unit) = remaining.first() else {
                            jobs.push(Err(error.into()));
                            return jobs;
                        };
                        let error = WorkError::at(error, unit);
                        let continue_run = execution.keep_going && error.recoverable();
                        jobs.push(Err(error));
                        if !continue_run {
                            return jobs;
                        }
                        position += 1;
                        continue;
                    }
                }
            }
            Err(error) => {
                let Some(unit) = remaining.first() else {
                    jobs.push(Err(error.into()));
                    return jobs;
                };
                let error = WorkError::at(error, unit);
                let continue_run = execution.keep_going && error.recoverable();
                jobs.push(Err(error));
                if !continue_run {
                    return jobs;
                }
                position += 1;
                continue;
            }
        };
        let end = match position.checked_add(packed.bindings.len()) {
            Some(end) => end,
            None => {
                jobs.push(Err(
                    Error::new(ErrorKind::Input, "window cursor overflow").into()
                ));
                return jobs;
            }
        };
        let Some(members) = units.get(position..end) else {
            jobs.push(Err(Error::new(
                ErrorKind::Unavailable,
                "window members out of range",
            )
            .into()));
            return jobs;
        };
        jobs.push(Ok(Job::Window {
            units: members.to_vec(),
            packed: Box::new(packed),
        }));
        position = end;
    }
    jobs
}

async fn evaluate_job(
    job: WorkResult<Job>,
    execution: Arc<Execution>,
) -> Vec<WorkResult<Completed>> {
    let recovery = execution.recovery.clone();
    let results = match job {
        Ok(Job::Saved(record)) => vec![Ok(*record)],
        Err(error) => vec![Err(error)],
        Ok(Job::Units(records)) => evaluate_records(records, execution).await,
        Ok(Job::Window { units, packed }) => evaluate_window(units, *packed, execution).await,
        Ok(Job::Split(job)) => {
            let unit = job.unit.clone();
            vec![
                evaluate_split(*job, execution)
                    .await
                    .map_err(|error| WorkError::at(error, &unit)),
            ]
        }
        Ok(Job::View(record)) => {
            let unit = record.unit.clone();
            vec![
                evaluate_view(*record, &execution)
                    .await
                    .map_err(|error| WorkError::at(error, &unit)),
            ]
        }
        Ok(Job::FocusPlan(record)) => vec![Ok(*record)],
        Ok(Job::FocusFinal(job)) => evaluate_focus_final(*job, &execution).await,
    };
    if let Some(recovery) = recovery
        && let Err(error) = recovery.capture(&results)
    {
        return vec![Err(error.into())];
    }
    results
}

fn unit_request(unit: &Unit, execution: &Execution) -> Request {
    Request {
        state: unit.state.clone(),
        model: execution.model.clone(),
        questions: execution
            .questions
            .iter()
            .map(|(id, question)| {
                (
                    id.clone(),
                    pack::bind(question, id, None, execution.contextual),
                )
            })
            .collect(),
    }
}

fn oversize_job(
    unit: Unit,
    request: Request,
    counts: Counts,
    path: &str,
    modes: IndexMap<String, CombineMode>,
    packed_indices: Option<Vec<u64>>,
    execution: &Execution,
) -> Result<Job> {
    let encoder = execution.encoder.as_deref().ok_or_else(|| {
        Error::new(
            ErrorKind::Usage,
            "explicit text views require a verified tokenizer",
        )
    })?;
    if let Some(settings) = &execution.truncate {
        let planned = truncate::plan(encoder, &request, path, settings.mode, &execution.limits)?;
        let mut view = serde_json::to_value(&planned.view).map_err(json_error)?;
        view["source"] = json!(unit.location);
        let answer_id = planned
            .request
            .questions
            .first()
            .map(|(id, _)| id.clone())
            .ok_or_else(|| Error::new(ErrorKind::Usage, "truncate question missing"))?;
        return Ok(Job::View(Box::new(Completed {
            unit,
            request: Arc::new(planned.request),
            counts: Arc::new(planned.counts),
            evaluation: None,
            answer_id,
            packed_indices,
            split_plan: None,
            view: Some(view),
            focus_plan: None,
            fallback: None,
            ensemble: None,
            review: None,
        })));
    }
    let settings = execution
        .split
        .as_ref()
        .ok_or_else(|| Error::new(ErrorKind::Usage, "split settings missing"))?;
    let chunks = split::plan(
        encoder,
        &request,
        path,
        &execution.limits,
        &settings.options,
    )
    .map_err(|error| Error::new(error.kind, format!("unit {}: {error}", unit.index)))?;
    Ok(Job::Split(Box::new(SplitJob {
        unit,
        request: Arc::new(request),
        counts: Arc::new(counts),
        chunks,
        modes,
        packed_indices,
        path: path.to_owned(),
    })))
}

fn view_path(execution: &Execution) -> Result<&str> {
    execution
        .truncate
        .as_ref()
        .map(|settings| settings.path.as_str())
        .or_else(|| {
            execution
                .split
                .as_ref()
                .map(|settings| settings.path.as_str())
        })
        .ok_or_else(|| Error::new(ErrorKind::Usage, "explicit text view is missing"))
}
fn validate_view_target(unit: &Unit, execution: &Execution) -> Result<()> {
    if execution.truncate.is_some()
        && unit
            .state
            .pointer(view_path(execution)?)
            .and_then(Value::as_str)
            .is_none()
    {
        return Err(Error::new(
            ErrorKind::Usage,
            format!(
                "unit {}: truncate target must be an existing string field",
                unit.index
            ),
        ));
    }
    Ok(())
}
async fn evaluate_view(mut record: Completed, execution: &Execution) -> Result<Completed> {
    if let Some(backend) = &execution.backend {
        execution.record_plan(
            vec![record.unit.index],
            &record.request,
            &record.counts,
            json!({"operation":"truncate","view":record.view}),
        )?;
        let mut evaluation = backend
            .evaluate(
                &record.request,
                &record.counts,
                &execution.budget,
                execution.strict,
                execution.deadline,
            )
            .await?;
        for answer in evaluation.response.answers.values_mut() {
            answer["lossy"] = json!(true);
            answer["provenance"] = record
                .view
                .clone()
                .ok_or_else(|| Error::new(ErrorKind::Input, "truncate provenance missing"))?;
        }
        record.evaluation = Some(Arc::new(evaluation));
    }
    Ok(record)
}

async fn evaluate_split(job: SplitJob, execution: Arc<Execution>) -> Result<Completed> {
    let plan = json!({"operation":"split","path":job.path,"coordinate_space":"original target string UTF-8 bytes and tokens","combine":job.modes,"weight":"unique source tokens; noul mean and choice vote are unweighted","source":job.unit.location,"chunks":job.chunks});
    let evaluation = if let Some(backend) = &execution.backend {
        execution.record_plan(
            vec![job.unit.index],
            &job.request,
            &job.counts,
            plan.clone(),
        )?;
        let execution = execution.as_ref();
        let responses = futures::stream::iter(job.chunks.iter())
            .map(|chunk| async move {
                backend
                    .evaluate(
                        &chunk.request,
                        &chunk.counts,
                        &execution.budget,
                        execution.strict,
                        execution.deadline,
                    )
                    .await
            })
            .buffered(execution.concurrency)
            .try_collect::<Vec<_>>()
            .await
            .map_err(|error| {
                Error::new(
                    error.kind,
                    format!("unit {} split: {error}", job.unit.index),
                )
            })?;
        let model = responses
            .first()
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "split has no response"))?
            .response
            .model
            .clone();
        let mut usage = Usage {
            input_tokens: 0,
            output_tokens: 0,
        };
        for response in &responses {
            if response.response.model != model {
                return Err(Error::new(
                    ErrorKind::Unavailable,
                    "model identity changed between split chunks",
                ));
            }
            usage.input_tokens = usage
                .input_tokens
                .checked_add(response.response.usage.input_tokens)
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "split input usage overflow"))?;
            usage.output_tokens = usage
                .output_tokens
                .checked_add(response.response.usage.output_tokens)
                .ok_or_else(|| Error::new(ErrorKind::Unavailable, "split output usage overflow"))?;
        }
        let mut answers = IndexMap::new();
        for (id, question) in &job.request.questions {
            let mode = job
                .modes
                .get(id)
                .ok_or_else(|| Error::new(ErrorKind::Usage, "split reducer missing"))?;
            let values = responses
                .iter()
                .zip(&job.chunks)
                .map(|(evaluation, chunk)| {
                    let value = evaluation.response.answers.get(id).ok_or_else(|| {
                        Error::new(ErrorKind::Unavailable, "split answer missing")
                    })?;
                    let weight = u64::try_from(chunk.range.unique_tokens)
                        .map_err(|_| Error::new(ErrorKind::Input, "split weight overflow"))?;
                    Ok((value.clone(), weight))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut answer = mode.apply(question, &values)?;
            answer["derived"] = json!(true);
            answer["provenance"] = json!({"operation":"split","path":job.path,"coordinate_space":"original target string UTF-8 bytes and tokens","combine":mode,"weight":"unique source tokens; noul mean and choice vote are unweighted","source":job.unit.location,"ranges":job.chunks.iter().map(|chunk|&chunk.range).collect::<Vec<_>>(),"responses":responses.iter().map(|evaluation|&evaluation.response).collect::<Vec<_>>()});
            answers.insert(id.clone(), answer);
        }
        let warning = responses
            .iter()
            .filter_map(|evaluation| evaluation.warning.as_deref())
            .collect::<Vec<_>>()
            .join("; ");
        Some(Arc::new(Evaluation {
            response: Response {
                model,
                answers,
                usage,
            },
            warning: if warning.is_empty() {
                None
            } else {
                Some(warning)
            },
            cached: responses.iter().all(|evaluation| evaluation.cached),
        }))
    } else {
        None
    };
    let answer_id = job
        .request
        .questions
        .first()
        .map(|(id, _)| id.clone())
        .ok_or_else(|| Error::new(ErrorKind::Usage, "split question missing"))?;
    Ok(Completed {
        unit: job.unit,
        request: job.request,
        counts: job.counts,
        evaluation,
        answer_id,
        packed_indices: job.packed_indices,
        split_plan: Some(plan),
        view: None,
        focus_plan: None,
        fallback: None,
        ensemble: None,
        review: None,
    })
}

async fn evaluate_window(
    units: Vec<Unit>,
    packed: pack::Packed,
    execution: Arc<Execution>,
) -> Vec<WorkResult<Completed>> {
    let indices = packed
        .bindings
        .iter()
        .map(|binding| binding.index)
        .collect::<Vec<_>>();
    let request = Arc::new(packed.request);
    let counts = Arc::new(packed.counts);
    let evaluation = if let Some(backend) = &execution.backend {
        if let Err(error)=execution.record_plan(indices.clone(),&request,&counts,json!({"operation":"window","bindings":packed.bindings.iter().map(|binding|json!({"index":binding.index,"source_id":binding.source_id,"questions":binding.questions.iter().collect::<Vec<_>>()})).collect::<Vec<_>>() })) {return vec![Err(error.into())];}
        match backend
            .evaluate(
                &request,
                &counts,
                &execution.budget,
                execution.strict,
                execution.deadline,
            )
            .await
        {
            Ok(evaluation) => Some(Arc::new(evaluation)),
            Err(error) => {
                return units
                    .iter()
                    .map(|unit| {
                        Err(WorkError::at(
                            Error::new(
                                error.kind,
                                format!("unit {} in window {indices:?}: {error}", unit.index),
                            ),
                            unit,
                        ))
                    })
                    .collect();
            }
        }
    } else {
        None
    };
    let mut output = Vec::new();
    for (unit, binding) in units.into_iter().zip(packed.bindings) {
        let answer_id = match binding.questions.first() {
            Some((_, id)) => id.clone(),
            None => {
                return vec![Err(WorkError::at(
                    Error::new(ErrorKind::Unavailable, "window answer binding missing"),
                    &unit,
                ))];
            }
        };
        output.push(Ok(Completed {
            unit,
            request: request.clone(),
            counts: counts.clone(),
            evaluation: evaluation.clone(),
            answer_id,
            packed_indices: Some(indices.clone()),
            split_plan: None,
            view: None,
            focus_plan: None,
            fallback: None,
            ensemble: None,
            review: None,
        }));
        if execution.backend.is_none() {
            break;
        }
    }
    output
}

fn prepare(
    kind: Kind,
    options: &Classify,
    store: &Store,
    limits: &loupe_core::config::Limits,
    model: &str,
) -> Result<Prepared> {
    review::validate(kind, options)?;
    if kind != Kind::Featurize
        && options
            .output_format
            .is_some_and(|output| matches!(output, Output::Csv | Output::Parquet))
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "CSV output requires featurize",
        ));
    }
    if options.why && matches!(kind, Kind::Ask | Kind::Tokens | Kind::Count) {
        return Err(Error::new(
            ErrorKind::Usage,
            "--why applies to per-unit decisions and is; ask/tokens expose raw fields and count is aggregate output",
        ));
    }
    if kind == Kind::Ask && options.min_agreement.is_some() {
        return Err(Error::new(
            ErrorKind::Usage,
            "raw ask responses do not apply --min-agreement; use a per-unit decision command",
        ));
    }
    if kind == Kind::Count
        && options.on_uncertain.is_some()
        && !matches!(options.on_uncertain, Some(Uncertain::Fallback))
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "count records an uncertain histogram bucket; --on-uncertain requires a per-unit output command",
        ));
    }
    let mut files = options.operands.clone();
    let loaded = if let Some(name) = &options.preset {
        Some(store.lookup(name)?)
    } else if let Some(path) = &options.question_file {
        Some(store.explicit(path)?)
    } else {
        None
    };
    if let Some(preset) = &loaded {
        preset.require_trusted()?;
        preset.validate(limits, model)?;
    }
    let mut document = if let Some(preset) = &loaded {
        preset.document.clone()
    } else {
        let question = if kind == Kind::Dedupe && (files.is_empty() || options.files_mode) {
            "Do left.record and right.record convey duplicate meaning about the same underlying item or event? Shared vocabulary alone is insufficient; distinct events are not duplicates.".to_owned()
        } else if files.is_empty() && kind == Kind::Tokens {
            "Is this state relevant?".to_owned()
        } else if files.is_empty() {
            return Err(Error::new(
                ErrorKind::Usage,
                "provide a positional question, -p preset or -f question file",
            ));
        } else {
            files.remove(0)
        };
        let question_type = match kind {
            Kind::Tag | Kind::Route => "choice",
            Kind::On if options.criteria.is_some() || options.criteria_file.is_some() => "choice",
            Kind::Rate | Kind::Sort => "score",
            _ => "noul",
        };
        json!({"type":question_type,"instructions":question})
    };
    if !document.is_object() {
        return Err(Error::new(
            ErrorKind::Usage,
            "question files and presets must be objects",
        ));
    }
    if let Some(criteria) = &options.criteria {
        let entries: Vec<_> = criteria.split(',').map(str::trim).collect();
        if entries.iter().any(|entry| entry.is_empty()) {
            return Err(Error::new(ErrorKind::Usage, "criteria cannot be empty"));
        }
        let criteria = if document["type"] == "score" {
            Value::Array(entries.iter().map(|entry| json!(entry)).collect())
        } else {
            let mut criteria = serde_json::Map::new();
            for entry in entries {
                if criteria.insert(entry.to_owned(), json!(entry)).is_some() {
                    return Err(Error::new(ErrorKind::Usage, "duplicate criteria key"));
                }
            }
            Value::Object(criteria)
        };
        document["criteria"] = criteria;
    }
    if let Some(path) = &options.criteria_file {
        let criteria = presets::read_document(path)?;
        document["criteria"] = criteria.get("criteria").unwrap_or(&criteria).clone();
    }
    let args = options
        .arg
        .iter()
        .map(|arg| {
            arg.split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .ok_or_else(|| Error::new(ErrorKind::Usage, "--arg requires NAME=VALUE"))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    if loupe_core::calibration_pin::validate(&document)?.is_some() && options.context.is_some() {
        return Err(Error::new(
            ErrorKind::Usage,
            "additional CLI context changes the calibrated question set; recalibrate with that context",
        ));
    }
    if let Some(mut preset) = loaded {
        preset.document = document;
        document = templates::expand(&preset, store, &args)?;
    } else {
        let preset = Preset {
            name: "inline".to_owned(),
            document,
            origin: loupe_core::preset_store::Origin::Explicit(store.project_root.clone()),
            trusted: true,
            content_hash: String::new(),
        };
        document = templates::expand(&preset, store, &args)?;
    }
    let mut questions: IndexMap<String, Question> = if matches!(kind, Kind::Ask | Kind::Featurize)
        || kind == Kind::Tokens && document.get("questions").is_some()
    {
        if (kind == Kind::Ask && options.question_file.is_none())
            || (options.question_file.is_none() && options.preset.is_none())
            || options.criteria.is_some()
            || options.criteria_file.is_some()
        {
            return Err(Error::new(
                ErrorKind::Usage,
                "ask requires a multi-question -f file; featurize requires a multi-question file or preset; criteria come from the source",
            ));
        }
        serde_json::from_value(
            document
                .get("questions")
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::Usage,
                        "multi-question source requires a questions map",
                    )
                })?
                .clone(),
        )
        .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?
    } else {
        IndexMap::from([(
            options
                .preset
                .clone()
                .unwrap_or_else(|| "decision".to_owned()),
            presets::question(&document)?,
        )])
    };
    if questions.is_empty() {
        return Err(Error::new(
            ErrorKind::Usage,
            "at least one primary question is required",
        ));
    }
    let factors = loupe_core::factors::Set::from_document(&document)?;
    factors.append(&mut questions)?;
    let ensembles = if options.ensemble {
        let set = loupe_core::ensemble::Set::from_document(&document, &questions)?;
        if set.is_empty() {
            return Err(Error::new(
                ErrorKind::Usage,
                "--ensemble requires named preset variants",
            ));
        }
        set.append(&mut questions)?;
        set
    } else {
        loupe_core::ensemble::Set::default()
    };
    let min_agreement = loupe_core::ensemble::minimum(&document, options.min_agreement)?;
    if kind == Kind::Featurize && options.ensemble {
        for (id, question) in ensembles.primary(&questions) {
            Policy::from_field(&document, &id, question.kind())?.validate()?;
        }
    }
    let signals = loupe_core::signals::Set::from_document(&document)?;
    if kind == Kind::Explain && factors.is_empty() {
        return Err(Error::new(
            ErrorKind::Usage,
            "explain requires a preset or question file with weighted factors",
        ));
    }
    if !factors.is_empty() && !matches!(kind, Kind::Explain | Kind::Ask | Kind::Tokens) {
        return Err(Error::new(
            ErrorKind::Usage,
            "weighted factor presets apply to explain, ask or tokens",
        ));
    }
    if kind == Kind::Explain && loupe_core::calibration_pin::validate(&document)?.is_some() {
        return Err(Error::new(
            ErrorKind::Usage,
            "explain composite indices require their own calibration; primary classification pins do not apply",
        ));
    }
    if options.why && signals.is_empty() {
        return Err(Error::new(
            ErrorKind::Usage,
            "--why requires declared preset/question-file signals",
        ));
    }
    signals.append(&mut questions)?;
    let static_context = document.get("context").cloned();
    let context = options
        .context
        .as_ref()
        .map(|path| {
            std::fs::read_to_string(path)
                .map(Value::String)
                .map_err(|error| Error::io("reading context", error))
        })
        .transpose()?;
    if static_context.is_some() || context.is_some() {
        let context = match (static_context, context) {
            (Some(preset), Some(cli)) => json!({"preset":preset,"cli":cli}),
            (Some(value), None) | (None, Some(value)) => value,
            (None, None) => Value::Null,
        };
        presets::add_context(&mut questions, context);
    }
    for (id, question) in &questions {
        question.validate()?;
        if signals.get(id).is_some()
            || factors.contains(id)
            || ensembles
                .base_for(id)
                .is_some_and(|base| factors.contains(base))
        {
            continue;
        }
        let valid = match kind {
            Kind::Is | Kind::Keep | Kind::Drop | Kind::Dedupe => {
                question.kind() == QuestionKind::Noul
            }
            Kind::Tag | Kind::Route => question.kind() == QuestionKind::Choice,
            Kind::Rate | Kind::Sort => question.kind() == QuestionKind::Score,
            Kind::On => question.kind() != QuestionKind::Score,
            Kind::Count => question.kind() != QuestionKind::Score,
            Kind::Pick => question.kind() != QuestionKind::Choice,
            Kind::Featurize => question.kind() != QuestionKind::Choice,
            _ => true,
        };
        if !valid {
            return Err(Error::new(
                ErrorKind::Usage,
                "question type is incompatible with command",
            ));
        }
        for (flag, provided) in [
            (PolicyFlag::Threshold, options.threshold.is_some()),
            (PolicyFlag::Margin, options.margin.is_some()),
            (PolicyFlag::MinConfidence, options.min_confidence.is_some()),
            (PolicyFlag::MinGap, options.min_gap.is_some()),
        ] {
            if provided {
                flag.validate(question.kind())?;
            }
        }
    }
    if matches!(
        kind,
        Kind::Ask | Kind::Tokens | Kind::Featurize | Kind::Explain
    ) && (options.threshold.is_some()
        || options.margin.is_some()
        || options.min_confidence.is_some()
        || options.min_gap.is_some()
        || options.on_uncertain.is_some()
        || options.uncertain.is_some())
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "raw ask/token plans and numeric features do not apply decision-policy flags",
        ));
    }
    if kind == Kind::Is
        && (options.on_uncertain.is_some()
            && !matches!(options.on_uncertain, Some(Uncertain::Fallback))
            || options.uncertain.is_some())
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "is reports uncertainty through exit 2",
        ));
    }
    let mut policy = if matches!(kind, Kind::Ask | Kind::Featurize | Kind::Explain) {
        Policy::default()
    } else {
        Policy::from_document(
            &document,
            questions
                .values()
                .next()
                .ok_or_else(|| Error::new(ErrorKind::Usage, "question missing"))?
                .kind(),
        )?
    };
    if let Some(value) = options.threshold {
        policy.threshold = Probability::new(value)
            .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
    }
    if let Some(value) = options.margin {
        policy.margin = value;
    }
    if let Some(value) = options.min_confidence {
        policy.min_confidence = Probability::new(value)
            .map_err(|error| Error::new(ErrorKind::Usage, error.to_string()))?;
    }
    if let Some(value) = options.min_gap {
        policy.min_gap = value;
    }
    policy.validate()?;
    limits.validate_schema(&Request {
        state: Value::Null,
        model: model.to_owned(),
        questions: questions.clone(),
    })?;
    let uncertain = match options.on_uncertain {
        Some(Uncertain::Keep) => UncertainAction::Keep,
        Some(Uncertain::Drop) => UncertainAction::Drop,
        Some(Uncertain::Fail) => UncertainAction::Fail,
        Some(Uncertain::Stderr) => UncertainAction::Stderr,
        Some(Uncertain::Fallback) => UncertainAction::Drop,
        None => match document
            .get("policy")
            .and_then(|value| value.get("on_uncertain"))
            .and_then(Value::as_str)
        {
            Some("keep") => UncertainAction::Keep,
            Some("fail") => UncertainAction::Fail,
            Some("stderr") => UncertainAction::Stderr,
            Some("drop" | "fallback") | None => UncertainAction::Drop,
            Some(_) => {
                return Err(Error::new(
                    ErrorKind::Usage,
                    "invalid preset uncertainty action",
                ));
            }
        },
    };
    let fallback = match options.on_uncertain {
        Some(Uncertain::Fallback) => true,
        Some(_) => false,
        None => {
            document
                .get("policy")
                .and_then(|policy| policy.get("on_uncertain"))
                .and_then(Value::as_str)
                == Some("fallback")
        }
    };
    if (fallback
        || options.fallback.is_some()
        || options.fallback_timeout.is_some()
        || options.on_fallback_error.is_some())
        && !matches!(
            kind,
            Kind::Is
                | Kind::Keep
                | Kind::Drop
                | Kind::Tag
                | Kind::Route
                | Kind::Rate
                | Kind::Sort
                | Kind::Count
                | Kind::On
                | Kind::Dedupe
                | Kind::Pick
        )
    {
        return Err(Error::new(
            ErrorKind::Usage,
            "this command does not support process fallback",
        ));
    }
    let ensembles = ensembles.materialize(&questions)?;
    Ok(Prepared {
        questions,
        files,
        policy,
        uncertain,
        fallback,
        document,
        signals,
        ensembles,
        min_agreement,
    })
}

fn read_units(
    mut files: Vec<String>,
    options: InputOptions,
    capacity: usize,
    recovery: Option<recovery::Registry>,
    feature_ids: Option<(bool, SourceKind)>,
) -> Result<tokio::sync::mpsc::Receiver<Result<Unit>>> {
    if files.is_empty() {
        files.push("-".to_owned());
    }
    if files.iter().filter(|file| file.as_str() == "-").count() > 1 {
        return Err(Error::new(
            ErrorKind::Usage,
            "stdin may be specified only once",
        ));
    }
    let (sender, receiver) = tokio::sync::mpsc::channel(capacity);
    std::thread::spawn(move || {
        let report = |error: Error| {
            if let Some(registry) = &recovery {
                let _ = registry.failure(None, &error);
            }
            let _ = sender.blocking_send(Err(error));
        };
        let mut replay = match recovery
            .as_ref()
            .map(recovery::Registry::reader)
            .transpose()
        {
            Ok(replay) => replay,
            Err(error) => {
                report(error);
                return;
            }
        };
        let mut fingerprints = Vec::new();
        let mut offset = 0;
        for file in files {
            let input: Result<Box<dyn BufRead>> = if file == "-" {
                Ok(Box::new(BufReader::new(io::stdin())))
            } else {
                File::open(&file)
                    .map(|file| Box::new(BufReader::new(file)) as Box<dyn BufRead>)
                    .map_err(|error| Error::io("opening input file", error))
            };
            let input = match input {
                Ok(input) => input,
                Err(error) => {
                    report(error);
                    return;
                }
            };
            let source = if file == "-" {
                None
            } else {
                Some(PathBuf::from(&file))
            };
            let fingerprint = recovery::Fingerprint::default();
            let input = if recovery.is_some() {
                Box::new(recovery::HashReader::new(input, fingerprint.clone())) as Box<dyn BufRead>
            } else {
                input
            };
            let mut reader = match Source::new(input, options.clone(), source) {
                Ok(reader) => reader,
                Err(error) => {
                    report(error);
                    return;
                }
            };
            loop {
                if recovery.as_ref().is_some_and(recovery::Registry::stopped) {
                    return;
                }
                match reader.next_unit() {
                    Ok(Some(mut unit)) => {
                        let Some(index) = unit.index.checked_add(offset) else {
                            report(Error::new(ErrorKind::Input, "source unit index overflow"));
                            return;
                        };
                        unit.index = index;
                        if let Some((selected, source)) = feature_ids
                            && let Err(error) =
                                loupe_core::features::row_id(&unit, selected, source)
                        {
                            report(error);
                            return;
                        }
                        let sent = match &mut replay {
                            Some(replay) => replay.unit(unit, &sender),
                            None => Ok(sender.blocking_send(Ok(unit)).is_ok()),
                        };
                        match sent {
                            Ok(true) => {}
                            Ok(false) => return,
                            Err(error) => {
                                report(error);
                                return;
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        report(error);
                        return;
                    }
                }
            }
            let Some(next) = offset.checked_add(reader.consumed_units()) else {
                report(Error::new(ErrorKind::Input, "source unit index overflow"));
                return;
            };
            offset = next;
            if recovery.is_some() {
                match fingerprint.value(&file, reader.consumed_units()) {
                    Ok(value) => fingerprints.push(value),
                    Err(error) => {
                        report(error);
                        return;
                    }
                }
            }
        }
        if let Some(replay) = &mut replay
            && let Err(error) = replay.end(json!({"sources":fingerprints}), &sender)
        {
            report(error);
        }
    });
    Ok(receiver)
}

pub(crate) fn duration(value: &str) -> Result<Duration> {
    let (number, multiplier) = if let Some(value) = value.strip_suffix("ms") {
        (value, 0.001)
    } else if let Some(value) = value.strip_suffix('s') {
        (value, 1.0)
    } else if let Some(value) = value.strip_suffix('m') {
        (value, 60.0)
    } else {
        (value, 1.0)
    };
    let seconds = number.parse::<f64>().map_err(|_| {
        Error::new(
            ErrorKind::Usage,
            "deadline must be a duration such as 30s or 250ms",
        )
    })? * multiplier;
    Duration::try_from_secs_f64(seconds)
        .map_err(|_| Error::new(ErrorKind::Usage, "deadline must be finite and nonnegative"))
}

fn selected(assessment: &Assessment) -> String {
    match &assessment.selected {
        Selected::Boolean(yes) => if *yes { "yes" } else { "no" }.to_owned(),
        Selected::Choice(label) => label.clone(),
        Selected::Score(score) => format!("{score:.4}"),
    }
}

fn emit(
    writer: &mut impl Write,
    completed: &Completed,
    assessment: &Assessment,
    output: Output,
    kind: Kind,
    why: Option<Why<'_>>,
) -> Result<()> {
    let response = &completed
        .evaluation
        .as_ref()
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing response"))?
        .response;
    let raw = response
        .answers
        .get(&completed.answer_id)
        .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing raw answer"))?;
    match output {
        Output::Csv | Output::Parquet => Err(Error::new(
            ErrorKind::Usage,
            "CSV output requires featurize",
        )),
        Output::Json => {
            let mut value = json!({"index":completed.unit.index,"unit":completed.unit.subject,"answer":raw,"model":response.model,"cached":completed.evaluation.as_ref().is_some_and(|evaluation|evaluation.cached)});
            if let Some(ensemble) = &completed.ensemble {
                value["ensemble"] = ensemble.clone();
                value["decision"] = json!({"selected":assessment.selected,"source":"ensemble","confidence":assessment.confidence,"uncertain":assessment.uncertain});
            }
            if let Some(fallback) = &completed.fallback {
                value["decision"] = loupe_core::fallback::decision(fallback);
                value["fallback"] = serde_json::to_value(fallback.as_ref()).map_err(json_error)?;
            }
            if let Some(review) = &completed.review
                && !review["task"].is_null()
            {
                value["review"] = review.clone();
                if crate::run::review::human(Some(review))?.is_some() {
                    value["decision"] = json!({"selected":assessment.selected,"source":"review","confidence":null,"uncertain":false});
                }
            }
            if let Some(why) = why {
                value["signals"] =
                    serde_json::to_value(why.reports(completed)?).map_err(json_error)?;
            }
            write_json(writer, &value)
        }
        Output::Tsv => writeln!(
            writer,
            "{}\t{}\t{}\t{}",
            completed.unit.index,
            selected(assessment),
            if matches!(
                completed.fallback.as_deref(),
                Some(loupe_core::fallback::Execution::Resolved { .. })
            ) || crate::run::review::human(completed.review.as_ref())?.is_some()
            {
                String::new()
            } else {
                assessment.confidence.get().to_string()
            },
            completed
                .unit
                .text
                .replace('\\', "\\\\")
                .replace('\t', "\\t")
                .replace('\r', "\\r")
                .replace('\n', "\\n")
        )
        .map_err(output_error),
        Output::Plain | Output::Annotate => {
            if matches!(kind, Kind::Tag | Kind::Rate) || output == Output::Annotate {
                let label = if crate::run::review::human(completed.review.as_ref())?.is_some()
                    && matches!(kind, Kind::Keep | Kind::Drop | Kind::Is)
                {
                    format!("review={}", selected(assessment))
                } else if matches!(
                    completed.fallback.as_deref(),
                    Some(loupe_core::fallback::Execution::Resolved { .. })
                ) && matches!(kind, Kind::Keep | Kind::Drop | Kind::Is)
                {
                    format!("fallback={}", selected(assessment))
                } else if matches!(kind, Kind::Keep | Kind::Drop | Kind::Is) {
                    if let Some(ensemble) = &completed.ensemble {
                        format!("ensemble_p={}", ensemble["mean_noul"])
                    } else {
                        format!(
                            "p={}",
                            raw.get("noul").and_then(Value::as_f64).unwrap_or(0.0)
                        )
                    }
                } else {
                    selected(assessment)
                };
                write!(writer, "{label}\t").map_err(output_error)?;
            }
            if kind != Kind::Is || output != Output::Plain {
                writer
                    .write_all(&completed.unit.raw)
                    .map_err(output_error)?;
            }
            Ok(())
        }
    }
}

fn json_error(error: serde_json::Error) -> Error {
    Error::new(ErrorKind::Input, error.to_string())
}
fn output_error(error: io::Error) -> Error {
    Error::io("writing output", error)
}

fn reject_input_output_alias(path: &Path, files: &[String]) -> Result<()> {
    let output = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::io("checking uncertainty output", error)),
    };
    let names: Vec<_> = if files.is_empty() {
        vec!["-"]
    } else {
        files.iter().map(String::as_str).collect()
    };
    for name in names {
        let source = if name == "-" {
            Path::new("/dev/fd/0")
        } else {
            Path::new(name)
        };
        if let Ok(input) = std::fs::metadata(source) {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if input.dev() == output.dev() && input.ino() == output.ino() {
                    return Err(Error::new(
                        ErrorKind::Usage,
                        "uncertainty output must differ from every input file",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn append_header(
    file: &mut File,
    unit: &Unit,
    format: Format,
    emitted: &mut Option<Vec<u8>>,
) -> Result<()> {
    let Some(header) = &unit.header else {
        return Ok(());
    };
    if let Some(previous) = emitted {
        if previous != header {
            return Err(Error::new(
                ErrorKind::Input,
                "uncertainty output requires identical CSV/TSV headers",
            ));
        }
        return Ok(());
    }
    let len = file
        .metadata()
        .map_err(|error| Error::io("checking uncertainty output", error))?
        .len();
    if len == 0 {
        file.write_all(header).map_err(output_error)?;
    } else {
        file.seek(SeekFrom::Start(0))
            .map_err(|error| Error::io("reading uncertainty header", error))?;
        let existing = Source::new(
            BufReader::new(
                file.try_clone()
                    .map_err(|error| Error::io("reading uncertainty output", error))?,
            ),
            InputOptions::new(format),
            None,
        )?;
        if existing.header() != Some(header.as_slice()) {
            return Err(Error::new(
                ErrorKind::Input,
                "existing uncertainty output has a different CSV/TSV header",
            ));
        }
        file.seek(SeekFrom::End(-1))
            .map_err(|error| Error::io("checking uncertainty delimiter", error))?;
        let mut last = [0];
        file.read_exact(&mut last)
            .map_err(|error| Error::io("checking uncertainty delimiter", error))?;
        if !matches!(last[0], b'\n' | b'\r') {
            return Err(Error::new(
                ErrorKind::Input,
                "existing CSV/TSV uncertainty output is missing its final record delimiter",
            ));
        }
    }
    *emitted = Some(header.clone());
    Ok(())
}

fn emit_header(
    writer: &mut impl Write,
    unit: &Unit,
    output: Output,
    kind: Kind,
    emitted: &mut Option<Vec<u8>>,
) -> Result<()> {
    if matches!(output, Output::Plain | Output::Annotate)
        && matches!(kind, Kind::Keep | Kind::Drop | Kind::Sort)
        && let Some(header) = &unit.header
    {
        match emitted {
            Some(previous) if previous != header => {
                return Err(Error::new(
                    ErrorKind::Input,
                    "CSV/TSV output requires identical headers across input files",
                ));
            }
            Some(_) => {}
            None => {
                writer.write_all(header).map_err(output_error)?;
                *emitted = Some(header.clone());
            }
        }
    }
    Ok(())
}
fn write_json(writer: &mut impl Write, value: &impl serde::Serialize) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).map_err(|error| {
        if let Some(kind) = error.io_error_kind() {
            output_error(io::Error::new(kind, error))
        } else {
            json_error(error)
        }
    })?;
    writer.write_all(b"\n").map_err(output_error)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::args::Output;
    use crate::run::{Completed, Kind, emit};
    use loupe_core::backend::Evaluation;
    use loupe_core::input::{Location, Unit};
    use loupe_core::policy::Policy;
    use loupe_core::wire::{Request, Response};

    #[test]
    fn arithmetic_ensemble_writers_keep_raw_answers_and_emit_mean_decisions() {
        use loupe_core::policy::{Probability, Selected};
        use serde_json::json;
        for (kind, question, primary, alternate, selected, annotation) in [
            (
                Kind::Keep,
                json!({"type":"noul"}),
                json!({"type":"noul","noul":0.2}),
                json!({"type":"noul","noul":1.0}),
                Selected::Boolean(true),
                "ensemble_p=0.6",
            ),
            (
                Kind::Tag,
                json!({"type":"choice","criteria":{"a":"A","b":"B"}}),
                json!({"type":"choice","choice":"a","confidence":0.7,"probabilities":{"a":0.7,"b":0.3}}),
                json!({"type":"choice","choice":"b","confidence":1.0,"probabilities":{"a":0.0,"b":1.0}}),
                Selected::Choice("b".into()),
                "b",
            ),
            (
                Kind::Rate,
                json!({"type":"score","criteria":["low","high"]}),
                json!({"type":"score","score":0.0,"confidence":1.0,"legend":{"0":"low","1":"high"},"probabilities":{"0":1.0,"1":0.0}}),
                json!({"type":"score","score":1.0,"confidence":1.0,"legend":{"0":"low","1":"high"},"probabilities":{"0":0.0,"1":1.0}}),
                Selected::Score(0.5),
                "0.5000",
            ),
        ] {
            // These are pure codec values, never submitted as model responses.
            let mut questions = indexmap::IndexMap::from([(
                "primary".into(),
                serde_json::from_value(question).unwrap(),
            )]);
            let set = loupe_core::ensemble::Set::from_document(
                &json!({"variants":[{"id":"direct"}]}),
                &questions,
            )
            .unwrap();
            set.append(&mut questions).unwrap();
            let unit = loupe_core::records::Source::new(
                std::io::Cursor::new(b"Arithmetic codec fixture\r\n"),
                loupe_core::records::Options::new(loupe_core::records::Format::Lines),
                None,
            )
            .unwrap()
            .next_unit()
            .unwrap()
            .unwrap();
            let request = Request {
                model: "mathematical-fixture".into(),
                state: unit.state.clone(),
                questions,
            };
            let response = Response {
                model: request.model.clone(),
                answers: indexmap::IndexMap::from([
                    ("primary".into(), primary.clone()),
                    ("ensemble:primary/direct".into(), alternate),
                ]),
                usage: loupe_core::wire::Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            };
            let report = set
                .report(
                    "primary",
                    "primary",
                    &request,
                    &response,
                    Policy::default(),
                    Probability::new(0.5).unwrap(),
                )
                .unwrap();
            let assessment = report.assessment.clone();
            assert_eq!(assessment.selected, selected);
            let report = serde_json::to_value(report).unwrap();
            let completed = Completed {
                unit,
                counts: std::sync::Arc::new(loupe_core::encoder::estimate(&request).unwrap()),
                request: std::sync::Arc::new(request),
                evaluation: Some(std::sync::Arc::new(Evaluation {
                    response,
                    warning: None,
                    cached: false,
                })),
                answer_id: "primary".into(),
                packed_indices: None,
                split_plan: None,
                view: None,
                focus_plan: None,
                fallback: None,
                ensemble: Some(report.clone()),
                review: None,
            };
            let mut output = Vec::new();
            emit(
                &mut output,
                &completed,
                &assessment,
                Output::Json,
                kind,
                None,
            )
            .unwrap();
            let row: serde_json::Value = serde_json::from_slice(&output).unwrap();
            assert_eq!(row["answer"], primary);
            assert_eq!(row["ensemble"], report);
            assert_eq!(
                row["decision"]["selected"],
                serde_json::to_value(selected).unwrap()
            );
            assert_eq!(row["decision"]["source"], "ensemble");
            output.clear();
            emit(
                &mut output,
                &completed,
                &assessment,
                Output::Annotate,
                kind,
                None,
            )
            .unwrap();
            assert_eq!(
                output,
                format!("{annotation}\tArithmetic codec fixture\r\n").as_bytes()
            );
        }
    }

    #[tokio::test]
    async fn saved_real_answer_writers_preserve_bytes_raw_objects_and_fallback_provenance() {
        let saved: serde_json::Value = serde_json::from_str(include_str!(
            "../../loupe-core/testdata/captured-response.json"
        ))
        .unwrap();
        let mut request: Request = serde_json::from_value(saved["request"].clone()).unwrap();
        request.questions.retain(|id, _| id == "outage");
        let mut response: Response = serde_json::from_value(saved["response"].clone()).unwrap();
        response.answers.retain(|id, _| id == "outage");
        let assessment = response.validate(&request).unwrap()["outage"]
            .assessment(&Policy::default())
            .unwrap();
        let mut completed = Completed {
            unit: Unit {
                index: 42,
                text: "one\ttwo".to_owned(),
                raw: b"one\ttwo\r\n".to_vec(),
                subject: serde_json::json!("one\ttwo"),
                state: serde_json::json!("one\ttwo"),
                header: None,
                location: Location {
                    source: None,
                    byte_start: 0,
                    byte_end: 9,
                    line_start: 1,
                    line_end: 1,
                },
            },
            request: std::sync::Arc::new(request),
            answer_id: "outage".to_owned(),
            packed_indices: None,
            split_plan: None,
            view: None,
            focus_plan: None,
            fallback: None,
            ensemble: None,
            review: None,
            counts: std::sync::Arc::new(loupe_core::encoder::Counts {
                state_tokens: 1,
                question_tokens: 1,
                input_tokens: 2,
                exact: true,
            }),
            evaluation: Some(std::sync::Arc::new(Evaluation {
                response,
                warning: None,
                cached: false,
            })),
        };
        let mut plain = Vec::new();
        emit(
            &mut plain,
            &completed,
            &assessment,
            Output::Plain,
            Kind::Keep,
            None,
        )
        .unwrap();
        assert_eq!(plain, b"one\ttwo\r\n");
        let mut annotation = Vec::new();
        emit(
            &mut annotation,
            &completed,
            &assessment,
            Output::Annotate,
            Kind::Keep,
            None,
        )
        .unwrap();
        assert_eq!(annotation, b"p=0.9654\tone\ttwo\r\n");
        let mut tsv = Vec::new();
        emit(
            &mut tsv,
            &completed,
            &assessment,
            Output::Tsv,
            Kind::Keep,
            None,
        )
        .unwrap();
        assert_eq!(tsv, b"42\tyes\t0.9654\tone\\ttwo\n");
        let mut json = Vec::new();
        emit(
            &mut json,
            &completed,
            &assessment,
            Output::Json,
            Kind::Keep,
            None,
        )
        .unwrap();
        let record: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(record["answer"], saved["response"]["answers"]["outage"]);
        assert_eq!(record.as_object().unwrap().len(), 5);
        assert_eq!(record["unit"], "one\ttwo");
        let input = loupe_core::fallback::Input {
            unit: &completed.unit,
            request: &completed.request,
            response: &completed.evaluation.as_ref().unwrap().response,
            field: &completed.answer_id,
            policy: Policy {
                margin: 1.0,
                ..Policy::default()
            },
        };
        let execution = loupe_core::fallback::execute(
            "printf no",
            &input,
            tokio::time::Instant::now() + std::time::Duration::from_secs(2),
        )
        .await
        .unwrap();
        completed.fallback = Some(std::sync::Arc::new(execution));
        let mut derived = assessment.clone();
        derived.selected = loupe_core::policy::Selected::Boolean(false);
        derived.uncertain = false;
        let mut external_json = Vec::new();
        emit(
            &mut external_json,
            &completed,
            &derived,
            Output::Json,
            Kind::Drop,
            None,
        )
        .unwrap();
        let record: serde_json::Value = serde_json::from_slice(&external_json).unwrap();
        assert_eq!(record["answer"], saved["response"]["answers"]["outage"]);
        assert_eq!(
            record["decision"]["selected"],
            serde_json::json!({"boolean":false})
        );
        assert_eq!(record["decision"]["source"], "fallback");
        assert!(record["decision"]["confidence"].is_null());
        let mut external_tsv = Vec::new();
        emit(
            &mut external_tsv,
            &completed,
            &derived,
            Output::Tsv,
            Kind::Drop,
            None,
        )
        .unwrap();
        assert_eq!(external_tsv, b"42\tno\t\tone\\ttwo\n");
        let mut external_annotation = Vec::new();
        emit(
            &mut external_annotation,
            &completed,
            &derived,
            Output::Annotate,
            Kind::Drop,
            None,
        )
        .unwrap();
        assert_eq!(external_annotation, b"fallback=no\tone\ttwo\r\n");
    }
}
