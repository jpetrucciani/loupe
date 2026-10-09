use std::io::Write;

use clap::CommandFactory;
use loupe_core::error::{Error, ErrorKind, Result};

use crate::args::Cli;

pub(crate) fn render(path: &[String], writer: &mut impl Write) -> Result<()> {
    let mut grammar = Cli::command();
    grammar.build();
    let mut command = &grammar;
    let mut name = "loupe".to_owned();
    for segment in path {
        command = command.find_subcommand(segment).ok_or_else(|| {
            Error::new(
                ErrorKind::Usage,
                format!("unknown manual command: {name} {segment}"),
            )
        })?;
        name.push(' ');
        name.push_str(command.get_name());
    }
    clap_mangen::Man::new(command.clone().bin_name(name.clone()).display_name(name))
        .render(writer)
        .map_err(manual_error)?;
    if path.is_empty() {
        for child in grammar.get_subcommands() {
            branch(child, "loupe", writer)?;
        }
    }
    Ok(())
}

fn branch(command: &clap::Command, parent: &str, writer: &mut impl Write) -> Result<()> {
    if command.is_hide_set() || command.get_name() == "help" {
        return Ok(());
    }
    let name = format!("{parent} {}", command.get_name());
    writeln!(writer, ".SH \"{}\"", name.to_ascii_uppercase()).map_err(manual_error)?;
    let page = clap_mangen::Man::new(command.clone().bin_name(name.clone()));
    let mut sections = Vec::new();
    page.render_synopsis_section(&mut sections)
        .map_err(manual_error)?;
    page.render_description_section(&mut sections)
        .map_err(manual_error)?;
    page.render_options_section(&mut sections)
        .map_err(manual_error)?;
    let text = String::from_utf8(sections)
        .map_err(|error| Error::new(ErrorKind::Io, format!("encoding manual: {error}")))?;
    for line in text.lines() {
        if let Some(heading) = line.strip_prefix(".SH ") {
            writeln!(writer, ".SS {heading}")
        } else {
            writeln!(writer, "{line}")
        }
        .map_err(manual_error)?;
    }
    for child in command.get_subcommands() {
        branch(child, &name, writer)?;
    }
    Ok(())
}

fn manual_error(error: std::io::Error) -> Error {
    Error::io("writing manual", error)
}
