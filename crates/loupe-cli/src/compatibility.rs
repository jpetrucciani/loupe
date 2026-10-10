#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::Path;

use clap::{Command, CommandFactory, FromArgMatches};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::args::Cli;

const DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/compatibility/v0.1");

/// Updating is an explicit developer operation. Ordinary tests never write fixtures.
pub(crate) fn snapshot(name: &str, actual: &str) {
    let path = Path::new(DIRECTORY).join(name);
    if std::env::var("LOUPE_UPDATE_CONTRACTS").as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        actual,
        expected,
        "interface changed: {}. Review the difference before explicitly updating the versioned fixture",
        path.display()
    );
}

pub(crate) fn snapshot_json(name: &str, actual: &Value) {
    snapshot(
        name,
        &format!("{}\n", serde_json::to_string_pretty(actual).unwrap()),
    );
}

fn action(action: &clap::ArgAction) -> &'static str {
    match action {
        clap::ArgAction::Set => "set",
        clap::ArgAction::Append => "append",
        clap::ArgAction::SetTrue => "true",
        clap::ArgAction::SetFalse => "false",
        clap::ArgAction::Count => "count",
        clap::ArgAction::Help => "help",
        clap::ArgAction::HelpShort => "help-short",
        clap::ArgAction::HelpLong => "help-long",
        clap::ArgAction::Version => "version",
        _ => panic!("new Clap action needs an explicit contract name"),
    }
}

fn commands(command: &Command, parent: &str, grammar: &mut Vec<Value>, reference: &mut String) {
    if command.is_hide_set() || command.get_name() == "help" {
        return;
    }
    let path = if parent.is_empty() {
        command.get_name().to_owned()
    } else {
        format!("{parent} {}", command.get_name())
    };
    let mut display = command
        .clone()
        .bin_name(&path)
        .term_width(100)
        .color(clap::ColorChoice::Never);
    // Never snapshot environment values, including credentials or endpoint overrides.
    display = display.mut_args(|arg| arg.hide_env_values(true));
    reference.push_str(&format!("\n## `{path}`\n\n```text\n"));
    for line in display.render_long_help().to_string().lines() {
        reference.push_str(line.trim_end());
        reference.push('\n');
    }
    reference.push_str("\n```\n");
    let arguments: Vec<_> = command.get_arguments().map(|arg| {
        let range = arg.get_num_args();
        let mut conflicts: Vec<_> = command.get_arg_conflicts_with(arg).iter().map(|arg| arg.get_id().as_str()).collect();
        conflicts.sort_unstable();
        conflicts.dedup();
        let mut value = json!({
            "id":arg.get_id().as_str(), "short":arg.get_short(), "long":arg.get_long(),
            "short_aliases":arg.get_all_short_aliases().unwrap_or_default(),
            "long_aliases":arg.get_all_aliases().unwrap_or_default(),
            "action":action(arg.get_action()), "required":arg.is_required_set(),
            "global":arg.is_global_set(), "position":arg.get_index(),
            "values":range.map(|range| json!({"min":range.min_values(),"max":range.max_values()})),
            "value_names":arg.get_value_names().map(|names| names.iter().map(|name|name.as_str()).collect::<Vec<_>>()),
            "possible_values":arg.get_possible_values().iter().map(|value|value.get_name()).collect::<Vec<_>>(),
            "defaults":arg.get_default_values().iter().map(|value|value.to_string_lossy()).collect::<Vec<_>>(),
            "environment":arg.get_env().map(|value|value.to_string_lossy()),
            "delimiter":arg.get_value_delimiter(), "terminator":arg.get_value_terminator().map(|value|value.as_str()),
            "require_equals":arg.is_require_equals_set(), "allow_hyphen_values":arg.is_allow_hyphen_values_set(),
            "allow_negative_numbers":arg.is_allow_negative_numbers_set(),
            "trailing_values":arg.is_trailing_var_arg_set(), "last":arg.is_last_set(),
            "exclusive":arg.is_exclusive_set(), "ignore_case":arg.is_ignore_case_set(),
            "hidden":arg.is_hide_set(), "conflicts":conflicts,
            "help":arg.get_long_help().or_else(||arg.get_help()).map(ToString::to_string)
        });
        value.as_object_mut().unwrap().retain(|_, value| {
            !value.is_null() && value != &json!(false) && value != &json!([])
        });
        value
    }).collect();
    let groups: Vec<_> = command.get_groups().map(|group| {
        let mut copy = group.clone();
        json!({"id":group.get_id().as_str(),"arguments":group.get_args().map(|id|id.as_str()).collect::<Vec<_>>(),"required":group.is_required_set(),"multiple":copy.is_multiple()})
    }).collect();
    grammar.push(json!({"path":path,"aliases":command.get_all_aliases().collect::<Vec<_>>(),"about":command.get_long_about().or_else(||command.get_about()).map(ToString::to_string),"arguments":arguments,"groups":groups}));
    for child in command.get_subcommands() {
        commands(child, &path, grammar, reference);
    }
}

#[test]
fn compatibility_full_grammar_and_reference() {
    let mut command = Cli::command();
    command.build();
    let mut grammar = Vec::new();
    let mut reference = "# Complete CLI flag reference\n\nGenerated from the native Clap grammar. This is the v0.1 development contract; the 1.0 freeze is pending. Every declared command is included below. Shared flags repeat where accepted. Runtime applicability, custom value syntaxes and JSON contracts are described in [the reference index](../../../../../docs/cli-reference.md). Environment variable names are shown; their current values are always suppressed.\n".to_owned();
    commands(&command, "", &mut grammar, &mut reference);
    snapshot_json(
        "grammar.json",
        &json!({"contract_version":"v0.1","omitted_argument_fields":"null, false or empty arrays","commands":grammar}),
    );
    snapshot("flags.md", &reference);
}

fn without_environment(command: Command) -> Command {
    command
        .mut_args(|arg| arg.env(None::<&str>))
        .mut_subcommands(without_environment)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParserCase {
    name: String,
    arguments: Vec<String>,
    expect: String,
}

#[test]
fn compatibility_parser_matrix() {
    let cases: Vec<ParserCase> =
        serde_json::from_str(include_str!("../tests/compatibility/v0.1/parser.json")).unwrap();
    let mut differences = Vec::new();
    for case in cases {
        let result = without_environment(Cli::command()).try_get_matches_from(
            std::iter::once("loupe").chain(case.arguments.iter().map(String::as_str)),
        );
        let actual = match result {
            Ok(matches) => {
                let cli = Cli::from_arg_matches(&matches).unwrap();
                if let Err(error) = cli.validate_global_flags() {
                    if format!("{:?}", error.kind()) != case.expect {
                        differences.push(format!(
                            "{}: {:?}: expected {}, got {:?}",
                            case.name,
                            case.arguments,
                            case.expect,
                            error.kind()
                        ));
                    }
                    continue;
                }
                let mut path = "loupe".to_owned();
                let mut node = &matches;
                while let Some((name, child)) = node.subcommand() {
                    path.push(' ');
                    path.push_str(name);
                    node = child;
                }
                path
            }
            Err(error) => format!("{:?}", error.kind()),
        };
        if actual != case.expect {
            differences.push(format!(
                "{}: {:?}: expected {}, got {actual}",
                case.name, case.arguments, case.expect
            ));
        }
    }
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

fn resolved_values(command: &Command, matches: &clap::ArgMatches, parent: &str) -> Vec<Value> {
    let path = if parent.is_empty() {
        command.get_name().to_owned()
    } else {
        format!("{parent} {}", command.get_name())
    };
    let values: serde_json::Map<_, _> = command
        .get_arguments()
        .filter_map(|arg| {
            let values = matches.get_raw(arg.get_id().as_str())?;
            Some((
                arg.get_id().to_string(),
                json!(
                    values
                        .map(|value| value.to_string_lossy())
                        .collect::<Vec<_>>()
                ),
            ))
        })
        .collect();
    let mut levels = vec![json!({"path":path,"values":values})];
    if let Some((name, child)) = matches.subcommand() {
        levels.extend(resolved_values(
            command.find_subcommand(name).unwrap(),
            child,
            &path,
        ));
    }
    levels
}

#[test]
fn compatibility_resolved_defaults_and_ordered_actions() {
    let cases: Vec<ParserCase> =
        serde_json::from_str(include_str!("../tests/compatibility/v0.1/parser.json")).unwrap();
    let mut grammar = without_environment(Cli::command());
    grammar.build();
    let mut resolved = Vec::new();
    for case in cases
        .into_iter()
        .filter(|case| case.expect.starts_with("loupe "))
    {
        let matches = grammar
            .clone()
            .try_get_matches_from(
                std::iter::once("loupe").chain(case.arguments.iter().map(String::as_str)),
            )
            .unwrap();
        let cli = Cli::from_arg_matches(&matches).unwrap();
        cli.validate_global_flags().unwrap();
        let mut result = json!({"name":case.name,"quiet":cli.quiet,"verbosity":cli.verbose,"levels":resolved_values(&grammar,&matches,"")});
        if matches.subcommand_name() == Some("on") {
            result["actions"] = serde_json::to_value(
                crate::args::on_declarations(matches.subcommand_matches("on").unwrap()).unwrap(),
            )
            .unwrap();
        }
        resolved.push(result);
    }
    snapshot_json("parsed-values.json", &json!(resolved));
}

#[test]
fn compatibility_error_exit_codes() {
    use loupe_core::error::{Error, ErrorKind};
    let codes = [
        ("usage", ErrorKind::Usage),
        ("input", ErrorKind::Input),
        ("unavailable", ErrorKind::Unavailable),
        ("authentication", ErrorKind::Auth),
        ("configuration", ErrorKind::Config),
        ("io", ErrorKind::Io),
        ("budget", ErrorKind::Budget),
        ("uncertain", ErrorKind::Uncertain),
        ("interrupted", ErrorKind::Interrupted),
    ]
    .into_iter()
    .map(|(name, kind)| {
        (
            name.to_owned(),
            json!(Error::new(kind, "exit mapping").exit_code()),
        )
    })
    .collect::<serde_json::Map<_, _>>();
    snapshot_json("errors.json", &Value::Object(codes));
}
