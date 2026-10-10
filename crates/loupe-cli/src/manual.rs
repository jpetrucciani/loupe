use std::io::Write;

use clap::CommandFactory;
use clap_mangen::roff::{Roff, bold, roman};
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
    let page = clap_mangen::Man::new(
        command
            .clone()
            .bin_name(name.clone())
            .display_name(name.clone()),
    )
    .source(format!("loupe {}", env!("CARGO_PKG_VERSION")));
    page.render_title(writer).map_err(manual_error)?;
    writeln!(writer, ".nh\n.ad l").map_err(manual_error)?;
    page.render_name_section(writer).map_err(manual_error)?;
    page.render_synopsis_section(writer).map_err(manual_error)?;
    page.render_description_section(writer)
        .map_err(manual_error)?;
    page.render_options_section(writer).map_err(manual_error)?;
    let mut index = Roff::default();
    let mut children = command
        .get_subcommands()
        .filter(|child| !child.is_hide_set() && child.get_name() != "help")
        .peekable();
    if children.peek().is_some() {
        index.control("SH", ["COMMANDS"]);
    }
    for child in children {
        index.control("TP", std::iter::empty::<&str>());
        index.text([bold(format!("{name} {}", child.get_name()))]);
        if let Some(about) = child.get_about() {
            index.text([roman(about.to_string())]);
        }
    }
    index.to_writer(writer).map_err(manual_error)?;
    if command.get_version().is_some() {
        page.render_version_section(writer).map_err(manual_error)?;
    }
    if command.get_author().is_some() {
        page.render_authors_section(writer).map_err(manual_error)?;
    }
    for child in command.get_subcommands() {
        branch(child, &name, writer)?;
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

#[cfg(test)]
mod tests {
    #[test]
    fn complete_manual_indexes_command_sections_without_missing_man_pages()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut bytes = Vec::new();
        crate::manual::render(&[], &mut bytes)?;
        let manual = String::from_utf8(bytes)?;
        assert!(manual.contains(".SH COMMANDS"));
        assert!(manual.contains(".SH \"LOUPE KEEP\""));
        assert!(manual.contains(".SH \"LOUPE PRESETS TRUST\""));
        assert!(!manual.contains("loupe\\-keep(1)"));
        Ok(())
    }
}
