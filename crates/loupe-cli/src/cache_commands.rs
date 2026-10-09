use std::io::{self, Write};
use std::path::Path;

use loupe_core::cache::{self, Stats, Store};
use loupe_core::error::{Error, ErrorKind, Result};

use crate::args::Cache;

pub fn execute(command: &Cache, directory: &Path) -> Result<u8> {
    match command {
        Cache::Stats { json } => {
            let stats = match Store::open(directory, true)? {
                Some(store) => store.stats()?,
                None => Stats::default(),
            };
            if *json {
                let mut bytes = serde_json::to_vec(&stats)
                    .map_err(|error| Error::new(ErrorKind::Io, error.to_string()))?;
                bytes.push(b'\n');
                io::stdout()
                    .lock()
                    .write_all(&bytes)
                    .map_err(|error| Error::io("writing cache stats", error))?;
            } else {
                writeln!(
                    io::stdout().lock(),
                    "entries={} payload_bytes={} database_bytes={} version={} present={}",
                    stats.entries,
                    stats.payload_bytes,
                    stats.database_bytes,
                    stats.version,
                    stats.present
                )
                .map_err(|error| Error::io("writing cache stats", error))?;
            }
        }
        Cache::Prune { older_than } => {
            let seconds = age(older_than)?;
            let cutoff = cache::now()?.saturating_sub(seconds);
            let removed = if directory
                .join(cache::FILE_NAME)
                .try_exists()
                .map_err(|error| Error::io("checking cache", error))?
            {
                match Store::open(directory, false)? {
                    Some(store) => store.prune(cutoff)?,
                    None => 0,
                }
            } else {
                0
            };
            writeln!(io::stdout().lock(), "pruned={removed}")
                .map_err(|error| Error::io("writing cache prune", error))?;
        }
        Cache::Clear => {
            Store::clear_directory(directory)?;
            writeln!(io::stdout().lock(), "cleared")
                .map_err(|error| Error::io("writing cache clear", error))?;
        }
    }
    Ok(0)
}
fn age(value: &str) -> Result<u64> {
    let boundary = value
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(boundary);
    let number = number.parse::<u64>().map_err(|_| {
        Error::new(
            ErrorKind::Usage,
            "older-than must be an integer duration such as 30d",
        )
    })?;
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => {
            return Err(Error::new(
                ErrorKind::Usage,
                "older-than units are s, m, h or d",
            ));
        }
    };
    number
        .checked_mul(multiplier)
        .ok_or_else(|| Error::new(ErrorKind::Usage, "older-than duration overflow"))
}
