mod args;
mod cache_commands;
#[cfg(test)]
mod compatibility;
mod manual;
mod preset_commands;
mod route;
mod run;
mod summary;

use clap::{CommandFactory, FromArgMatches};
use std::io::{self, Write};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let matches = match args::Cli::command().try_get_matches() {
        Ok(matches) => matches,
        Err(error) => {
            let code = if error.use_stderr() { 64 } else { 0 };
            let _ = error.print();
            return ExitCode::from(code);
        }
    };
    let mut cli = match args::Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(64);
        }
    };
    if let Err(error) = cli.validate_global_flags() {
        let _ = error.print();
        return ExitCode::from(64);
    }
    if let args::Command::On(options) = &mut cli.command {
        let ordered = matches
            .subcommand_matches("on")
            .ok_or_else(|| {
                loupe_core::error::Error::new(
                    loupe_core::error::ErrorKind::Usage,
                    "on arguments missing",
                )
            })
            .and_then(args::on_declarations);
        match ordered {
            Ok(ordered) => options.ordered = ordered,
            Err(error) => {
                let _ = writeln!(io::stderr().lock(), "loupe: {error}");
                return ExitCode::from(error.exit_code());
            }
        }
    }
    match run::execute(cli).await {
        Ok(code) => ExitCode::from(code),
        Err(error) if error.is_broken_pipe() => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "loupe: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
