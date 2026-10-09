use crate::args::Dedupe;
use loupe_core::error::{Error, Result};
use loupe_core::recovery::Store;
use serde_json::Value;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(crate) fn open(options: &Dedupe, identity: Value, files: &[String]) -> Result<Option<Store>> {
    let Some(path) = options
        .options
        .failed
        .as_deref()
        .or(options.options.resume.as_deref())
    else {
        return Ok(None);
    };
    let mut protected = Vec::new();
    for path in files
        .iter()
        .filter(|file| file.as_str() != "-")
        .map(Path::new)
        .chain([Path::new("/dev/fd/0"), Path::new("/dev/fd/1")])
    {
        match std::fs::metadata(path) {
            Ok(metadata) => protected.push((metadata.dev(), metadata.ino())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(Error::io("checking dedupe journal aliases", error)),
        }
    }
    if options.options.resume.is_some() {
        Store::resume(path, &identity, &protected).map(Some)
    } else {
        Store::create(path, identity, &protected).map(Some)
    }
}
