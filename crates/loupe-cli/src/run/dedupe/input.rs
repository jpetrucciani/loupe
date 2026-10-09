use crate::run::recovery::{Fingerprint, HashReader};
use loupe_core::error::{Error, ErrorKind, Result};
use loupe_core::input::Unit;
use loupe_core::records::{Options, Source};
use serde_json::{Value, json};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;

pub(crate) struct Catalog {
    pub(crate) units: Vec<Unit>,
    pub(crate) fingerprints: Value,
}
pub(crate) async fn read(mut files: Vec<String>, options: Options) -> Result<Catalog> {
    if files.is_empty() {
        files.push("-".into());
    }
    if files.iter().filter(|file| file.as_str() == "-").count() > 1 {
        return Err(Error::new(
            ErrorKind::Usage,
            "stdin may be specified only once",
        ));
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let read = || -> Result<Catalog> {
            let mut units = Vec::new();
            let mut fingerprints = Vec::new();
            let mut offset = 0u64;
            let mut header = None;
            for file in files {
                let source = (file != "-").then(|| PathBuf::from(&file));
                let input: Box<dyn BufRead> = if file == "-" {
                    Box::new(BufReader::new(io::stdin()))
                } else {
                    Box::new(BufReader::new(
                        File::open(&file)
                            .map_err(|error| Error::io("opening dedupe input", error))?,
                    ))
                };
                let fingerprint = Fingerprint::default();
                let mut reader = Source::new(
                    HashReader::new(input, fingerprint.clone()),
                    options.clone(),
                    source,
                )?;
                if let Some(current) = reader.header() {
                    if header
                        .as_ref()
                        .is_some_and(|previous: &Vec<u8>| previous.as_slice() != current)
                    {
                        return Err(Error::new(
                            ErrorKind::Input,
                            "dedupe CSV/TSV files require identical headers",
                        ));
                    }
                    header = Some(current.to_vec());
                }
                while let Some(mut unit) = reader.next_unit()? {
                    unit.index = unit.index.checked_add(offset).ok_or_else(|| {
                        Error::new(ErrorKind::Input, "dedupe input index overflow")
                    })?;
                    units.push(unit);
                }
                fingerprints.push(fingerprint.value(&file, reader.consumed_units())?);
                offset = offset
                    .checked_add(reader.consumed_units())
                    .ok_or_else(|| Error::new(ErrorKind::Input, "dedupe input index overflow"))?;
            }
            Ok(Catalog {
                units,
                fingerprints: json!({"sources":fingerprints}),
            })
        };
        let _ = sender.send(read());
    });
    receiver
        .await
        .map_err(|_| Error::new(ErrorKind::Io, "dedupe input reader stopped"))?
}
