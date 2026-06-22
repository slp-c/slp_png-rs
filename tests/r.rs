use std::fs::File;

use slp_png_rs::{Image, read::imread};

pub const READ_PATH: &str = "test_images/10.4-MB.png";

#[test]
fn r() -> Result<(), ()> {
    let mut input: File = File::open(READ_PATH).map_err(|_| {})?;
    let _img: Image = imread(&mut input).ok_or(())?;
    return Ok(());
}
