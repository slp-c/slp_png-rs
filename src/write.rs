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
use std::{fs::File, io::Write, ptr};

use zlib_rs::{DeflateConfig, compress_bound, compress_slice, crc32::crc32};

use crate::{Image, div_round_up};

const PNG_SIGNATURE: u64 = 0x89504E470D0A1A0A;

impl Image {
    pub fn write(&self, file: &mut File) -> Option<()> {
        if self.width == 0
            || self.height == 0
            || self.channels == 0
            || (match self.bit_depth {
                1 | 2 | 4 | 8 | 16 => false,
                _ => true,
            })
        {
            return None;
        }

        let is_color_type_3: bool = !self.palette.is_empty();

        #[allow(non_camel_case_types)]
        #[repr(C, packed)]
        struct ihdr_t {
            width: u32,
            height: u32,
            bit_depth: u8,
            color_type: u8,
            compression_method: u8,
            filter_method: u8,
            interlace_method: u8,
        }
        let ihdr: ihdr_t = ihdr_t {
            width: self.width.to_be(),
            height: self.height.to_be(),
            bit_depth: self.bit_depth,
            color_type: get_color_type(self.channels, is_color_type_3)?,
            compression_method: 0,
            filter_method: 0,
            interlace_method: 0,
        };
        let ihdr: [u8; 13] = unsafe {
            (*ptr::slice_from_raw_parts(&ihdr as *const ihdr_t as *const u8, 13))
                .try_into()
                .unwrap()
        };

        file.write(&PNG_SIGNATURE.to_be_bytes()).ok()?;
        write_chunk(b"IHDR", &ihdr, file)?;

        if !self.palette.is_empty() {
            let (plte, trns) = make_plte_trns(&self.palette);
            write_chunk(b"PLTE", &plte, file)?;
            write_chunk(b"tRNS", &trns, file)?;
        }

        let idat: Vec<u8> = make_idat(self, is_color_type_3)?;
        write_chunk(b"IDAT", &idat, file)?;

        write_chunk(b"IEND", &[], file)?;
        return Some(());
    }
}

fn make_idat(img: &Image, is_color_type_3: bool) -> Option<Vec<u8>> {
    const LEVEL: i32 = 6;

    let channels: usize = match is_color_type_3 {
        true => 1,
        false => img.channels as usize,
    };

    let bpp: usize = channels * (1 + (img.bit_depth == 16) as usize);
    let bpr: usize = div_round_up(img.width as usize * img.bit_depth as usize * channels, 8);

    let mut filtered_image: Vec<u8> = vec![0; (bpr + 1) * img.height as usize];
    let mut filter_buf: [Vec<u8>; 5] = [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for i in 0..filter_buf.len() {
        filter_buf[i] = vec![0; bpr + 1];
        filter_buf[i][0] = i as u8;
    }

    for i in 0..img.height as usize {
        let filter_scores: [u64; 5] = filter_row(&mut filter_buf, &img.buffer, bpp, bpr, i);

        let mut best_filter_type = 0;
        for i in 0..filter_scores.len() {
            if filter_scores[i] < filter_scores[best_filter_type] {
                best_filter_type = i;
            }
        }

        filtered_image[i * (bpr + 1)..(i + 1) * (bpr + 1)]
            .copy_from_slice(&filter_buf[best_filter_type]);
    }

    let mut idat: Vec<u8> = vec![0; compress_bound(filtered_image.len())];
    let (compressed, rc) = compress_slice(
        &mut idat,
        &filtered_image,
        DeflateConfig {
            level: LEVEL,
            method: zlib_rs::Method::Deflated,
            window_bits: 15,
            mem_level: 8,
            strategy: zlib_rs::Strategy::Filtered,
        },
    );
    if rc != zlib_rs::ReturnCode::Ok {
        return None;
    }
    let len = compressed.len();
    idat.resize(len, 0);

    return Some(idat);
}

fn filter_row(dst: &mut [Vec<u8>; 5], src: &[u8], bpp: usize, bpr: usize, i: usize) -> [u64; 5] {
    let mut scores: [u64; 5] = [0; 5];
    if i == 0 {
        // choose the sub filter
        let raw: &[u8] = &src[i * bpr..(i + 1) * bpr];
        for j in 0..bpp {
            dst[1][j + 1] = raw[j];
        }
        for j in bpp..bpr {
            dst[1][j + 1] = raw[j].wrapping_sub(raw[j - bpp]);
        }
        for score in &mut scores[..] {
            *score = u64::MAX;
        }
        scores[1] = 0;
        return scores;
    }

    let raw: &[u8] = &src[i * bpr..(i + 1) * bpr];
    let up: &[u8] = &src[(i - 1) * bpr..i * bpr];

    for j in 0..bpp {
        dst[0][j + 1] = raw[j];
        dst[1][j + 1] = raw[j];
        dst[2][j + 1] = raw[j].wrapping_sub(up[j]);
        dst[3][j + 1] = raw[j].wrapping_sub(up[j] / 2);
        dst[4][j + 1] = raw[j].wrapping_sub(up[j]);

        scores[0] += (dst[0][j + 1] as i64).unsigned_abs();
        scores[1] += (dst[1][j + 1] as i64).unsigned_abs();
        scores[2] += (dst[2][j + 1] as i64).unsigned_abs();
        scores[3] += (dst[3][j + 1] as i64).unsigned_abs();
        scores[4] += (dst[4][j + 1] as i64).unsigned_abs();
    }

    for j in bpp..bpr {
        let a: i32 = raw[j - bpp] as i32;
        let b: i32 = up[j] as i32;
        let c: i32 = up[j - bpp] as i32;

        let p: i32 = a + b - c;
        let pa: i32 = (p - a).abs();
        let pb: i32 = (p - b).abs();
        let pc: i32 = (p - c).abs();

        let d: u8 = match pa <= pb && pa <= pc {
            true => a as u8,
            false => match pb <= pc {
                true => b as u8,
                false => c as u8,
            },
        };

        let avg: u8 = ((a + b) / 2) as u8;

        dst[0][j + 1] = raw[j];
        dst[1][j + 1] = raw[j].wrapping_sub(raw[j - bpp]);
        dst[2][j + 1] = raw[j].wrapping_sub(up[j]);
        dst[3][j + 1] = raw[j].wrapping_sub(avg);
        dst[4][j + 1] = raw[j].wrapping_sub(d);

        scores[0] += (dst[0][j + 1] as i64).unsigned_abs();
        scores[1] += (dst[1][j + 1] as i64).unsigned_abs();
        scores[2] += (dst[2][j + 1] as i64).unsigned_abs();
        scores[3] += (dst[3][j + 1] as i64).unsigned_abs();
        scores[4] += (dst[4][j + 1] as i64).unsigned_abs();

        if scores[0] >= u64::MAX - 0xFF
            || scores[1] >= u64::MAX - 0xFF
            || scores[2] >= u64::MAX - 0xFF
            || scores[3] >= u64::MAX - 0xFF
            || scores[4] >= u64::MAX - 0xFF
        {
            let base: u64 = *scores.iter().min().unwrap();
            for score in &mut scores[..] {
                *score -= base;
            }
        }
    }

    return scores;
}

fn write_chunk(chunk_type: &[u8; 4], data: &[u8], file: &mut File) -> Option<()> {
    let crc: u32 = crc32(crc32(0, chunk_type), data);
    file.write(&u32::to_be_bytes(data.len() as u32)).ok()?;
    file.write(chunk_type).ok()?;
    file.write(data).ok()?;
    file.write(&crc.to_be_bytes()).ok()?;
    Some(())
}

fn make_plte_trns(palette: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let entries = palette.len() / 4;
    let mut plte: Vec<u8> = vec![0; entries * 3];
    let mut trns: Vec<u8> = vec![0; entries];
    for i in 0..entries {
        plte[i * 3..i * 3 + 3].copy_from_slice(&palette[i * 4..i * 4 + 3]);
        trns[i] = palette[i * 4 + 3];
    }
    return (plte, trns);
}

fn get_color_type(channel: u8, is_color_type_3: bool) -> Option<u8> {
    if is_color_type_3 {
        return Some(3);
    }
    match channel {
        1 => return Some(0),
        2 => return Some(4),
        3 => return Some(2),
        4 => return Some(6),
        _ => return None,
    }
}
