use std::io::Write;

use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::features::{Schema, row_id};
use loupe_core::input::Unit;
use loupe_core::pack::SourceKind;
use serde_json::{Value, json};

use crate::args::Output as Format;
use crate::run::{Completed, Why, json_error, signal_text, write_json};

pub(crate) struct Output<'a> {
    pub(crate) schema: &'a Schema,
    pub(crate) format: Format,
    pub(crate) selected_id: bool,
    pub(crate) source: SourceKind,
    ensemble: Option<loupe_core::features::Ensemble<'a>>,
    #[cfg(feature = "parquet")]
    parquet: std::cell::RefCell<Option<loupe_core::features_parquet::Writer>>,
}

impl Output<'_> {
    pub(crate) fn new<'a>(
        schema: &'a Schema,
        format: Format,
        selected_id: bool,
        source: SourceKind,
        planning: bool,
        ensemble: Option<loupe_core::features::Ensemble<'a>>,
    ) -> Result<Output<'a>> {
        #[cfg(not(feature = "parquet"))]
        let _ = planning;
        Ok(Output {
            schema,
            format,
            selected_id,
            source,
            ensemble,
            #[cfg(feature = "parquet")]
            parquet: std::cell::RefCell::new(if format == Format::Parquet && !planning {
                Some(loupe_core::features_parquet::Writer::new(schema)?)
            } else {
                None
            }),
        })
    }

    pub(crate) fn finish(&self, writer: &mut impl Write) -> Result<()> {
        #[cfg(feature = "parquet")]
        if let Some(parquet) = self.parquet.borrow_mut().take() {
            return parquet.finish(writer);
        }
        #[cfg(not(feature = "parquet"))]
        let _ = writer;
        Ok(())
    }

    fn parquet(
        &self,
        writer: &mut impl Write,
        index: u64,
        id: &Value,
        values: Option<&indexmap::IndexMap<String, f64>>,
        error: Option<&str>,
    ) -> Result<()> {
        #[cfg(feature = "parquet")]
        return self
            .parquet
            .borrow_mut()
            .as_mut()
            .ok_or_else(|| Error::new(ErrorKind::Usage, "Parquet writer is unavailable"))?
            .push(writer, index, id, values, error);
        #[cfg(not(feature = "parquet"))]
        {
            let _ = (writer, index, id, values, error);
            Err(Error::new(
                ErrorKind::Usage,
                "Parquet requires --features parquet",
            ))
        }
    }
    pub(crate) fn emit(
        &self,
        writer: &mut impl Write,
        completed: &Completed,
        why: Option<Why<'_>>,
        quiet: bool,
    ) -> Result<()> {
        let evaluation = completed
            .evaluation
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::Unavailable, "missing feature response"))?;
        let values = self.schema.project(
            &completed.request,
            &evaluation.response,
            &completed.answer_id,
            self.ensemble,
        )?;
        let id = row_id(&completed.unit, self.selected_id, self.source)?;
        let signals = why.map(|why| why.reports(completed)).transpose()?;
        if !quiet && let Some(signals) = &signals {
            eprintln!(
                "loupe: unit {} features [{}]",
                completed.unit.index,
                signal_text(signals)
            );
        }
        match self.format {
            Format::Parquet => self.parquet(
                writer,
                completed.unit.index,
                &id,
                Some(&values.features),
                None,
            ),
            Format::Csv => self.schema.csv_row(
                writer,
                completed.unit.index,
                &id,
                Some(&values.features),
                None,
            ),
            Format::Json => {
                let mut value = json!({"index":completed.unit.index,"row_id":id,"unit":completed.unit.subject,"features":values.features,"answers":values.answers,"bindings":values.bindings,"model":evaluation.response.model,"cached":evaluation.cached,"location":completed.unit.location});
                if !values.ensembles.is_empty() {
                    value["ensembles"] =
                        serde_json::to_value(values.ensembles).map_err(json_error)?;
                }
                if let Some(signals) = signals {
                    value["signals"] = serde_json::to_value(signals).map_err(json_error)?;
                }
                if let Some(indices) = &completed.packed_indices {
                    value["indices"] = json!(indices);
                }
                if let Some(view) = &completed.view {
                    value["truncate"] = view.clone();
                }
                if let Some(split) = &completed.split_plan {
                    value["split"] = split.clone();
                }
                if let Some(focus) = &completed.focus_plan {
                    value["focus"] = focus.clone();
                }
                write_json(writer, &value)
            }
            _ => Err(Error::new(
                ErrorKind::Usage,
                "feature output requires csv or json",
            )),
        }
    }

    pub(crate) fn failure(
        &self,
        writer: &mut impl Write,
        unit: &Unit,
        error: &Error,
    ) -> Result<()> {
        let id = row_id(unit, self.selected_id, self.source)?;
        match self.format {
            Format::Parquet => {
                self.parquet(writer, unit.index, &id, None, Some(&error.to_string()))
            }
            Format::Csv => {
                self.schema
                    .csv_row(writer, unit.index, &id, None, Some(&error.to_string()))
            }
            Format::Json => write_json(
                writer,
                &json!({"index":unit.index,"row_id":id,"unit":unit.subject,"features":self.schema.columns().iter().map(|column|(column.clone(),Value::Null)).collect::<serde_json::Map<_,_>>(),"answers":null,"bindings":null,"model":null,"cached":false,"location":unit.location,"error":{"code":error.exit_code(),"message":error.to_string()}}),
            ),
            _ => Err(Error::new(
                ErrorKind::Usage,
                "feature output requires csv or json",
            )),
        }
    }
}
