//! Print a content/revision pin using the same confined reader as evaluation.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: suite-pin ROOT RELATIVE_FILE REVISION".into());
    }
    let root = args.first().ok_or("missing root")?;
    let path = args.get(1).ok_or("missing path")?;
    let revision = args
        .get(2)
        .and_then(|value| value.to_str())
        .ok_or("revision must be UTF-8")?;
    let pin = loupe_core::suites::Pin::capture(
        std::path::Path::new(root),
        std::path::Path::new(path),
        revision,
    )?;
    println!("{}", serde_json::to_string(&pin)?);
    Ok(())
}
