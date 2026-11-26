//! Own minimal PNG encoder (8-bit RGBA, zlib "stored" deflate blocks).

use crate::paint::Framebuffer;
use std::io::Write;

pub fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for n in 0..256u32 {
        let mut c = n;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        table[n as usize] = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

pub fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(kind);
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

/// Encode a framebuffer as a PNG into the given writer.
pub fn encode_png(fb: &Framebuffer, out: &mut dyn Write) -> std::io::Result<()> {
    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(fb.width as u32).to_be_bytes());
    ihdr.extend_from_slice(&(fb.height as u32).to_be_bytes());
    ihdr.push(8); // bit depth
    ihdr.push(6); // color type RGBA
    ihdr.push(0); // compression
    ihdr.push(0); // filter
    ihdr.push(0); // interlace
    chunk(&mut png, b"IHDR", &ihdr);

    // raw scanlines with filter byte 0
    let row_bytes = fb.width * 4;
    let mut raw = Vec::with_capacity((row_bytes + 1) * fb.height);
    for y in 0..fb.height {
        raw.push(0);
        raw.extend_from_slice(&fb.data[y * row_bytes..(y + 1) * row_bytes]);
    }

    // zlib stream: header + stored deflate blocks + adler32
    let mut z = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 11);
    z.push(0x78);
    z.push(0x01);
    let mut pos = 0;
    while pos < raw.len() {
        let take = (raw.len() - pos).min(65535);
        let last = pos + take == raw.len();
        z.push(if last { 0x01 } else { 0x00 });
        z.extend_from_slice(&(take as u16).to_le_bytes());
        z.extend_from_slice(&(!(take as u16)).to_le_bytes());
        z.extend_from_slice(&raw[pos..pos + take]);
        pos += take;
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);

    out.write_all(&png)
}

pub fn write_png(path: &str, fb: &Framebuffer) -> std::io::Result<()> {
    let mut f = std::fs::File::create(path)?;
    encode_png(fb, &mut f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        // CRC32 of "123456789" is 0xCBF43926
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn adler32_known_vector() {
        assert_eq!(adler32(b"Wikipedia"), 0x11E60398);
    }

    #[test]
    fn png_signature_and_roundtrip_via_zlib() {
        let mut fb = Framebuffer::new(4, 2);
        fb.clear(crate::css::Color::rgb(10, 20, 30));
        let mut buf = Vec::new();
        encode_png(&fb, &mut buf).unwrap();
        assert_eq!(&buf[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(buf.windows(4).any(|w| w == b"IEND"));
        // IHDR width/height
        assert_eq!(&buf[16..20], &4u32.to_be_bytes());
        assert_eq!(&buf[20..24], &2u32.to_be_bytes());
    }
}
