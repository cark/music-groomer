use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub(super) fn copy_contents(source: &Path, destination: &Path) -> io::Result<()> {
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    io::copy(&mut input, &mut output)?;
    Ok(())
}
