//! Incremental, uncompressed Parquet using the same numeric projection as CSV/JSON.

use std::io::Write;
use std::sync::Arc;

use indexmap::IndexMap;
use parquet::basic::{ConvertedType, Repetition, Type as Physical};
use parquet::data_type::{ByteArray, ByteArrayType, DoubleType, Int64Type};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::types::Type;
use serde_json::Value;

use crate::error::{Error, ErrorKind, Result};
use crate::features::Schema;

struct Row {
    index: i64,
    id: ByteArray,
    values: Option<Vec<f64>>,
    error: Option<ByteArray>,
}

pub struct Writer {
    file: SerializedFileWriter<Vec<u8>>,
    columns: Vec<String>,
    rows: Vec<Row>,
    bytes: usize,
}

impl Writer {
    pub fn new(schema: &Schema) -> Result<Self> {
        let columns = schema.columns().to_vec();
        let index = Type::primitive_type_builder("index", Physical::INT64)
            .with_repetition(Repetition::REQUIRED)
            .with_converted_type(ConvertedType::UINT_64)
            .build()
            .map_err(failure)?;
        let id = Type::primitive_type_builder("row_id", Physical::BYTE_ARRAY)
            .with_repetition(Repetition::REQUIRED)
            .with_converted_type(ConvertedType::UTF8)
            .build()
            .map_err(failure)?;
        let mut fields = vec![Arc::new(index), Arc::new(id)];
        for name in schema.columns() {
            fields.push(Arc::new(
                Type::primitive_type_builder(name, Physical::DOUBLE)
                    .with_repetition(Repetition::OPTIONAL)
                    .build()
                    .map_err(failure)?,
            ));
        }
        fields.push(Arc::new(
            Type::primitive_type_builder("error", Physical::BYTE_ARRAY)
                .with_repetition(Repetition::OPTIONAL)
                .with_converted_type(ConvertedType::UTF8)
                .build()
                .map_err(failure)?,
        ));
        let schema = Type::group_type_builder("loupe_features")
            .with_fields(fields)
            .build()
            .map_err(failure)?;
        let properties = WriterProperties::builder()
            .set_created_by(format!("loupe {}", env!("CARGO_PKG_VERSION")))
            .build();
        Ok(Self {
            file: SerializedFileWriter::new(Vec::new(), Arc::new(schema), Arc::new(properties))
                .map_err(failure)?,
            columns,
            rows: Vec::new(),
            bytes: 0,
        })
    }

    pub fn push(
        &mut self,
        output: &mut impl Write,
        index: u64,
        id: &Value,
        values: Option<&IndexMap<String, f64>>,
        error: Option<&str>,
    ) -> Result<()> {
        let id = id.as_str().map_or_else(|| id.to_string(), str::to_owned);
        let numeric = values
            .map(|values| {
                self.columns
                    .iter()
                    .map(|column| {
                        values
                            .get(column)
                            .copied()
                            .filter(|value| value.is_finite())
                            .ok_or_else(|| {
                                Error::new(
                                    ErrorKind::Unavailable,
                                    "missing/nonfinite Parquet feature",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()?;
        let bytes = id
            .len()
            .checked_add(error.map_or(0, str::len))
            .and_then(|bytes| bytes.checked_add(self.columns.len().checked_mul(8)?))
            .ok_or_else(|| Error::new(ErrorKind::Input, "Parquet row size overflow"))?;
        if bytes > 16 * 1024 * 1024 {
            return Err(Error::new(ErrorKind::Input, "Parquet row exceeds 16 MiB"));
        }
        if self.bytes.saturating_add(bytes) > 8 * 1024 * 1024 {
            self.flush_group(output)?;
        }
        self.bytes += bytes;
        self.rows.push(Row {
            index: i64::from_ne_bytes(index.to_ne_bytes()),
            id: ByteArray::from(id.as_str()),
            values: numeric,
            error: error.map(ByteArray::from),
        });
        if self.rows.len() >= 256 {
            self.flush_group(output)?;
        }
        Ok(())
    }

    fn drain(&mut self, output: &mut impl Write) -> Result<()> {
        self.file
            .flush()
            .map_err(|error| Error::io("flushing Parquet", error))?;
        output
            .write_all(self.file.inner())
            .map_err(|error| Error::io("writing Parquet", error))?;
        self.file.inner_mut().clear();
        Ok(())
    }

    fn flush_group(&mut self, output: &mut impl Write) -> Result<()> {
        if self.rows.is_empty() {
            return Ok(());
        }
        let mut group = self.file.next_row_group().map_err(failure)?;
        let mut index = group
            .next_column()
            .map_err(failure)?
            .ok_or_else(|| failure("missing index column"))?;
        index
            .typed::<Int64Type>()
            .write_batch(
                &self.rows.iter().map(|row| row.index).collect::<Vec<_>>(),
                None,
                None,
            )
            .map_err(failure)?;
        index.close().map_err(failure)?;
        let mut id = group
            .next_column()
            .map_err(failure)?
            .ok_or_else(|| failure("missing row_id column"))?;
        id.typed::<ByteArrayType>()
            .write_batch(
                &self
                    .rows
                    .iter()
                    .map(|row| row.id.clone())
                    .collect::<Vec<_>>(),
                None,
                None,
            )
            .map_err(failure)?;
        id.close().map_err(failure)?;
        for position in 0..self.columns.len() {
            let mut column = group
                .next_column()
                .map_err(failure)?
                .ok_or_else(|| failure("missing feature column"))?;
            let levels = self
                .rows
                .iter()
                .map(|row| i16::from(row.values.is_some()))
                .collect::<Vec<_>>();
            let values = self
                .rows
                .iter()
                .filter_map(|row| {
                    row.values
                        .as_ref()
                        .and_then(|values| values.get(position))
                        .copied()
                })
                .collect::<Vec<_>>();
            column
                .typed::<DoubleType>()
                .write_batch(&values, Some(&levels), None)
                .map_err(failure)?;
            column.close().map_err(failure)?;
        }
        let mut error = group
            .next_column()
            .map_err(failure)?
            .ok_or_else(|| failure("missing error column"))?;
        let levels = self
            .rows
            .iter()
            .map(|row| i16::from(row.error.is_some()))
            .collect::<Vec<_>>();
        let values = self
            .rows
            .iter()
            .filter_map(|row| row.error.clone())
            .collect::<Vec<_>>();
        error
            .typed::<ByteArrayType>()
            .write_batch(&values, Some(&levels), None)
            .map_err(failure)?;
        error.close().map_err(failure)?;
        if group.next_column().map_err(failure)?.is_some() {
            return Err(failure("unexpected Parquet column"));
        }
        group.close().map_err(failure)?;
        self.rows.clear();
        self.bytes = 0;
        self.drain(output)
    }

    pub fn finish(mut self, output: &mut impl Write) -> Result<()> {
        self.flush_group(output)?;
        self.file.finish().map_err(failure)?;
        self.drain(output)
    }
}

fn failure(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Io, format!("Parquet: {error}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use crate::features::Schema;
    use crate::features_parquet::Writer;
    use crate::wire::Question;
    use indexmap::IndexMap;
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use parquet::record::RowAccessor;
    use serde_json::json;
    #[test]
    fn real_file_roundtrip_preserves_exact_features_utf8_ids_unsigned_indices_and_null_errors() {
        let schema = Schema::new(IndexMap::from([(
            "present".into(),
            Question::Noul {
                instructions: None,
                criteria: None,
            },
        )]))
        .unwrap();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("features.parquet");
        let mut file = std::fs::File::create(&path).unwrap();
        let mut writer = Writer::new(&schema).unwrap();
        for index in 0..257 {
            writer
                .push(
                    &mut file,
                    index,
                    &json!("é,\"line\nsecond"),
                    Some(&IndexMap::from([("p_present".into(), 0.12345678901234568)])),
                    None,
                )
                .unwrap();
        }
        writer
            .push(
                &mut file,
                u64::MAX,
                &json!(9007199254740993u64),
                None,
                Some("failed"),
            )
            .unwrap();
        writer.finish(&mut file).unwrap();
        drop(file);
        let reader = SerializedFileReader::new(std::fs::File::open(path).unwrap()).unwrap();
        assert_eq!(reader.metadata().file_metadata().num_rows(), 258);
        assert_eq!(reader.num_row_groups(), 2);
        let rows = reader
            .get_row_iter(None)
            .unwrap()
            .map(|row| row.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(rows[0].get_string(1).unwrap(), "é,\"line\nsecond");
        assert_eq!(rows[0].get_double(2).unwrap(), 0.12345678901234568);
        assert_eq!(rows[257].get_ulong(0).unwrap(), u64::MAX);
        assert_eq!(rows[257].get_string(1).unwrap(), "9007199254740993");
        assert!(rows[257].get_double(2).is_err());
        assert_eq!(rows[257].get_string(3).unwrap(), "failed");
    }
}
