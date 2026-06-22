pub mod read;
pub mod write;

#[derive(Debug, Default)]
pub struct Image {
    pub buffer: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub bit_depth: u8,
    pub palette: Vec<u8>,
}

pub(crate) fn read_be_u32(bytes: &[u8]) -> u32 {
    return u32::from_be_bytes(bytes[0..4].try_into().unwrap());
}

pub(crate) fn read_be_u64(bytes: &[u8]) -> u64 {
    return u64::from_be_bytes(bytes[0..8].try_into().unwrap());
}

pub(crate) fn div_round_up(a: usize, b: usize) -> usize {
    return a / b + (a % b != 0) as usize;
}
