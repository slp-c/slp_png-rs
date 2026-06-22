use std::fs::File;

use slp_png_rs::{Image, read::imread};

pub const READ_PATH: &str = "test_images/10.4-MB.png";
pub const WRITE_PATH: &str = "test_images/new.png";

#[test]
fn rw() -> Result<(), ()> {
    let mut input: File = File::open(READ_PATH).map_err(|_| {})?;
    let img: Image = imread(&mut input).ok_or(())?;

    /*
    //println!("Image:     /home/rei/Pictures/wallpaper/miyabi0.png");
    println!("Width:     {}", img.width);
    println!("Height:    {}", img.height);
    println!("Channel:   {}", img.channels);
    println!("Bit depth: {}", img.bit_depth);
    println!("Pixel:     {}", img.buffer.len());
    */

    let mut output: File = File::create(WRITE_PATH).map_err(|_| {})?;
    img.write(&mut output).ok_or(())?;

    return Ok(());
}
