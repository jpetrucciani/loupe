use std::io::Write;

use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use serde_json::json;

use crate::args::Output;
use crate::run::{Kind, output_error, write_json};

/// Only errors with a complete original unit can be handled by a per-unit policy.
pub(crate) struct WorkError {
    pub(crate) error: Error,
    pub(crate) unit: Option<Box<Unit>>,
}
pub(crate) type WorkResult<T> = std::result::Result<T, WorkError>;

impl WorkError {
    pub(crate) fn at(error: Error, unit: &Unit) -> Self {
        Self {
            error,
            unit: Some(Box::new(unit.clone())),
        }
    }
    pub(crate) fn recoverable(&self) -> bool {
        self.unit.is_some() && matches!(self.error.kind, ErrorKind::Input | ErrorKind::Unavailable)
    }
}
impl From<Error> for WorkError {
    fn from(error: Error) -> Self {
        Self { error, unit: None }
    }
}

pub(crate) fn emit(
    writer: &mut impl Write,
    unit: &Unit,
    error: &Error,
    output: Output,
    kind: Kind,
) -> Result<()> {
    match output {
        Output::Csv | Output::Parquet => Err(Error::new(
            ErrorKind::Usage,
            "CSV output requires feature schema",
        )),
        Output::Json => write_json(
            writer,
            &json!({"index":unit.index,"unit":unit.subject,"answer":null,"model":null,"cached":false,"error":{"code":error.exit_code(),"message":error.to_string()}}),
        ),
        Output::Tsv => writeln!(
            writer,
            "{}\terror\t\t{}",
            unit.index,
            unit.text
                .replace('\\', "\\\\")
                .replace('\t', "\\t")
                .replace('\r', "\\r")
                .replace('\n', "\\n")
        )
        .map_err(output_error),
        Output::Plain | Output::Annotate => {
            if output == Output::Annotate || matches!(kind, Kind::Tag | Kind::Rate) {
                writer.write_all(b"error\t").map_err(output_error)?;
            }
            writer.write_all(&unit.raw).map_err(output_error)
        }
    }
}
