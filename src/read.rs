/*
Copyright 2026 slp-c

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/
use std::{fs::File, io::Read};

use zlib_rs::{InflateConfig, crc32::crc32, decompress_slice};

use crate::{Image, div_round_up, read_be_u32, read_be_u64};

const PNG_SIGNATURE: u64 = 0x89504E470D0A1A0A;
const IHDR: u32 = u32::from_be_bytes(*b"IHDR");
const IDAT: u32 = u32::from_be_bytes(*b"IDAT");
const IEND: u32 = u32::from_be_bytes(*b"IEND");
const PLTE: u32 = u32::from_be_bytes(*b"PLTE");
#[allow(non_upper_case_globals)]
const tRNS: u32 = u32::from_be_bytes(*b"tRNS");

pub fn imread(file: &mut File) -> Option<Image> {
    let mut img: Image = Image::default();
    let mut buf: [u8; 33] = [0; 33];

    match file.read(&mut buf) {
        Ok(len) => {
            if len != 33 {
                return None;
            }
        }
        Err(_) => return None,
    }

    let mut crc: u32 = crc32(0, &buf[12..16]);
    crc = crc32(crc, &buf[16..29]);

    if read_be_u64(&buf[0..8]) != PNG_SIGNATURE
        || read_be_u32(&buf[8..12]) != 13
        || read_be_u32(&buf[12..16]) != IHDR
        || read_be_u32(&buf[29..33]) != crc
    {
        return None;
    }

    img.width = read_be_u32(&buf[16..20]);
    img.height = read_be_u32(&buf[20..24]);
    img.bit_depth = buf[24];
    let color_type: u8 = buf[25];
    img.channels = get_channel(color_type, img.bit_depth)?;
    if buf[26] != 0 || buf[27] != 0 || buf[28] != 0 {
        return None;
    }

    let bpp: usize = match color_type {
        3 => 1,
        _ => img.channels as usize * (1 + (img.bit_depth == 16) as usize),
    };
    let bpr: usize = match color_type {
        3 => div_round_up(img.width as usize * img.bit_depth as usize, 8),
        _ => div_round_up(
            img.width as usize * img.bit_depth as usize * img.channels as usize,
            8,
        ),
    };

    let parsed_data: Parsing = Parsing::parse(file)?;
    let filter_image: Vec<u8> = decompress_idat(&parsed_data.idat, &img, bpr)?;
    img.buffer = defilter(&filter_image, &img, bpp, bpr)?;

    if color_type == 3 {
        if parsed_data.plte.is_empty() {
            return None;
        }

        let entries: usize = parsed_data.plte.len() / 3;
        img.palette = vec![0xFF; entries * 4];

        for i in 0..entries {
            img.palette[i * 4..i * 4 + 3].copy_from_slice(&parsed_data.plte[i * 3..i * 3 + 3]);
            img.palette[i * 4 + 3] = if !parsed_data.trns.is_empty() && i < parsed_data.trns.len() {
                parsed_data.trns[i]
            } else {
                0xFF
            }
        }
    }

    return Some(img);
}

fn defilter(filter_image: &[u8], img: &Image, bpp: usize, bpr: usize) -> Option<Vec<u8>> {
    let image_size = bpr * img.height as usize;
    let mut buf: Vec<u8> = vec![0; image_size];

    let mut output = buf.chunks_exact_mut(bpr);

    let mut dst: &mut [u8] = output.next()?;
    match filter_image[0] {
        0 => defiler_by_row::none(&filter_image[1..bpr + 1], &mut dst, bpr),
        1 => defiler_by_row::sub(&filter_image[1..bpr + 1], &mut dst, bpp, bpr),
        2 => defiler_by_row::none(&filter_image[1..bpr + 1], &mut dst, bpr),
        3 => defiler_by_row::avg_for_line0(&filter_image[1..bpr + 1], &mut dst, bpp, bpr),
        4 => defiler_by_row::sub(&filter_image[1..bpr + 1], &mut dst, bpp, bpr),
        _ => return None,
    }
    let mut prev: &[u8] = dst;

    for curr in filter_image[bpr + 1..].chunks_exact(bpr + 1) {
        let filter = curr[0];
        let src: &[u8] = &curr[1..];
        dst = output.next()?;

        match filter {
            0 => defiler_by_row::none(src, &mut dst, bpr),
            1 => defiler_by_row::sub(src, &mut dst, bpp, bpr),
            2 => defiler_by_row::up(src, prev, &mut dst, bpr),
            3 => defiler_by_row::avg(src, prev, &mut dst, bpp, bpr),
            4 => defiler_by_row::paeth(src, prev, &mut dst, bpp, bpr),
            _ => return None,
        }
        prev = dst;
    }
    return Some(buf);
}

mod defiler_by_row {
    pub(crate) fn none(src: &[u8], dst: &mut [u8], bpr: usize) {
        dst[0..bpr].copy_from_slice(&src[0..bpr]);
    }
    pub(crate) fn sub(src: &[u8], dst: &mut [u8], bpp: usize, bpr: usize) {
        dst[0..bpp].copy_from_slice(&src[0..bpp]);
        for i in bpp..bpr {
            dst[i] = src[i].wrapping_add(dst[i - bpp]);
        }
    }
    pub(crate) fn up(src: &[u8], prev_dst: &[u8], dst: &mut [u8], bpr: usize) {
        for i in 0..bpr {
            dst[i] = src[i].wrapping_add(prev_dst[i]);
        }
    }
    pub(crate) fn avg_for_line0(src: &[u8], dst: &mut [u8], bpp: usize, bpr: usize) {
        dst[0..bpp].copy_from_slice(&src[0..bpp]);
        for i in bpp..bpr {
            dst[i] = src[i].wrapping_add(dst[i - bpp] / 2);
        }
    }
    pub(crate) fn avg(src: &[u8], prev_dst: &[u8], dst: &mut [u8], bpp: usize, bpr: usize) {
        for i in 0..bpp {
            dst[i] = src[i].wrapping_add(prev_dst[i] / 2);
        }
        for i in bpp..bpr {
            dst[i] = src[i].wrapping_add(((prev_dst[i] as u16 + dst[i - bpp] as u16) / 2) as u8);
        }
    }
    pub(crate) fn paeth(src: &[u8], prev_dst: &[u8], dst: &mut [u8], bpp: usize, bpr: usize) {
        for i in 0..bpp {
            dst[i] = src[i].wrapping_add(prev_dst[i]);
        }
        for i in bpp..bpr {
            let a: i32 = dst[i - bpp] as i32;
            let b: i32 = prev_dst[i] as i32;
            let c: i32 = prev_dst[i - bpp] as i32;

            let p: i32 = a + b - c;
            let pa: i32 = i32::abs(p - a);
            let pb: i32 = i32::abs(p - b);
            let pc: i32 = i32::abs(p - c);

            let d: u8 = match pa <= pb && pa <= pc {
                true => a as u8,
                false => match pb <= pc {
                    true => b as u8,
                    false => c as u8,
                },
            };

            dst[i] = src[i].wrapping_add(d);
        }
    }
}

fn decompress_idat(idat: &[u8], img: &Image, bpr: usize) -> Option<Vec<u8>> {
    let size: usize = (bpr + 1) * img.height as usize;
    let mut buf: Vec<u8> = vec![0; size];
    let (data, return_code) = decompress_slice(&mut buf, idat, InflateConfig::default());
    if data.len() != buf.len() || return_code != zlib_rs::ReturnCode::Ok {
        return None;
    }
    return Some(buf);
}

fn get_channel(color_type: u8, bit_depth: u8) -> Option<u8> {
    match color_type {
        0 => match bit_depth {
            1 | 2 | 4 | 8 | 16 => return Some(1),
            _ => return None,
        },
        2 => match bit_depth {
            8 | 16 => return Some(3),
            _ => return None,
        },
        3 => match bit_depth {
            1 | 2 | 4 | 8 => return Some(4),
            _ => return None,
        },
        4 => match bit_depth {
            8 | 16 => return Some(2),
            _ => return None,
        },
        6 => match bit_depth {
            8 | 16 => return Some(4),
            _ => return None,
        },
        _ => return None,
    }
}

#[derive(Default)]
struct Parsing {
    idat: Vec<u8>,
    plte: Vec<u8>,
    trns: Vec<u8>,
}

impl Parsing {
    fn parse(file: &mut File) -> Option<Parsing> {
        let mut parser: Parsing = Parsing::default();

        struct CheckList {
            idat: bool,
            iend: bool,
        }
        let mut check_list = CheckList {
            idat: false,
            iend: false,
        };

        parser.idat = Vec::new();
        loop {
            let mut chunk: Chunk = Chunk::parse_chunk(file)?;
            match chunk.chunk_type {
                IDAT => {
                    parser.idat.append(&mut chunk.data);
                    check_list.idat = true;
                }
                PLTE => {
                    if !parser.plte.is_empty() {
                        return None;
                    }
                    parser.plte = Vec::new();
                    parser.plte.append(&mut chunk.data);
                }
                #[allow(non_upper_case_globals)]
                tRNS => {
                    if !parser.trns.is_empty() {
                        return None;
                    }
                    parser.trns = Vec::new();
                    parser.trns.append(&mut chunk.data);
                }
                IEND => {
                    if !check_list.idat {
                        return None;
                    }
                    check_list.iend = true;
                    break;
                }
                _ => {}
            }
        }
        if !check_list.iend {
            return None;
        }
        return Some(parser);
    }
}

struct Chunk {
    chunk_type: u32,
    data: Vec<u8>,
}
impl Chunk {
    fn parse_chunk(file: &mut File) -> Option<Chunk> {
        let mut buf: [u8; 12] = [0; 12];
        file.read_exact(&mut buf[0..8]).ok()?;
        let chunk_len: u32 = read_be_u32(&buf[0..4]);
        let chunk_type: u32 = read_be_u32(&buf[4..8]);

        let mut chunk: Chunk = Chunk {
            chunk_type: chunk_type,
            data: vec![0; chunk_len as usize],
        };
        file.read_exact(&mut chunk.data).ok()?;
        file.read_exact(&mut buf[8..12]).ok()?;

        let crc: u32 = crc32(crc32(0, &buf[4..8]), &chunk.data);
        if crc != read_be_u32(&buf[8..12]) {
            return None;
        }
        return Some(chunk);
    }
}
