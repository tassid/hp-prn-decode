//! HP "Mode 10" raster row compression, as used by PCL3GUI colour planes.
//!
//! A row is a list of commands. Each command first skips some pixels (they stay
//! equal to the row above, the "seed row"), then replaces a run of pixels.
//! Anything after the last command also stays equal to the seed row.
//!
//! Command byte: `T SS OO CCC`
//! - `T`   0 = literal run, 1 = repeated (RLE) run
//! - `SS`  where the first pixel comes from: new colour, west, north-east, cached
//! - `OO`  pixels to skip (3 = more count bytes follow)
//! - `CCC` run length (7 = more count bytes follow)

pub type Pixel = [u8; 3];
pub const WHITE: Pixel = [255; 3];

const SOURCE_NEW: u8 = 0;
const SOURCE_WEST: u8 = 1;
const SOURCE_NORTH_EAST: u8 = 2;

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self.data.get(self.pos).ok_or("dados da linha acabaram no meio de um comando")?;
        self.pos += 1;
        Ok(byte)
    }

    /// Count continuation: keep adding bytes while they are 255.
    fn extend(&mut self, mut total: usize) -> Result<usize, String> {
        loop {
            let more = self.byte()?;
            total += more as usize;
            if more != 255 {
                return Ok(total);
            }
        }
    }

    /// A pixel written in full: either 3 bytes of RGB shifted right by one bit,
    /// or (high bit set) 2 bytes holding a small difference from the pixel above.
    fn pixel(&mut self, above: Pixel) -> Result<Pixel, String> {
        let first = self.byte()?;
        if first & 0x80 == 0 {
            let value = (u32::from(first) << 16 | u32::from(self.byte()?) << 8 | u32::from(self.byte()?)) << 1;
            let blue = value as u8;
            return Ok([(value >> 16) as u8, (value >> 8) as u8, blue | blue >> 7]);
        }
        let value = u16::from(first) << 8 | u16::from(self.byte()?);
        let signed5 = |bits: u16| ((bits & 0x1F) as i16) - if bits & 0x10 != 0 { 32 } else { 0 };
        let add = |base: u8, delta: i16| (i16::from(base) + delta) as u8;
        Ok([
            add(above[0], signed5(value >> 10)),
            add(above[1], signed5(value >> 5)),
            add(above[2], signed5(value) * 2),
        ])
    }
}

pub fn decode_row(data: &[u8], seed: &[Pixel]) -> Result<Vec<Pixel>, String> {
    let width = seed.len();
    let mut row = seed.to_vec();
    let mut reader = Reader { data, pos: 0 };
    let mut cached = WHITE;
    let mut x = 0;

    while reader.pos < data.len() {
        let command = reader.byte()?;
        let repeated = command & 0x80 != 0;
        let source = command >> 5 & 0b11;
        let skip = usize::from(command >> 3 & 0b11);
        let count = usize::from(command & 0b111);

        x += if skip == 3 { reader.extend(3)? } else { skip };
        if x >= width {
            return Err(format!("comando começa na coluna {x}, fora da largura {width}"));
        }

        let first = match source {
            SOURCE_NEW => {
                cached = reader.pixel(seed[x])?;
                cached
            }
            SOURCE_WEST => {
                if x == 0 {
                    WHITE
                } else {
                    row[x - 1]
                }
            }
            SOURCE_NORTH_EAST => seed.get(x + 1).copied().unwrap_or(WHITE),
            _ => cached,
        };

        if repeated {
            let run = if count == 7 { reader.extend(9)? } else { count + 2 };
            // The HP driver was observed ending a row with a run a few pixels past
            // the declared width, so the last run is clipped instead of rejected.
            let end = (x + run).min(width);
            if x + run > width && reader.pos < data.len() {
                return Err(format!("repetição passa da largura {width} antes do fim da linha"));
            }
            row[x..end].fill(first);
            x = end;
        } else {
            row[x] = first;
            x += 1;
            let mut remaining = count;
            let mut extended = count == 7;
            loop {
                for _ in 0..remaining {
                    if x >= width {
                        return Err(format!("literal passa da largura {width}"));
                    }
                    row[x] = reader.pixel(seed[x])?;
                    x += 1;
                }
                if !extended {
                    break;
                }
                let more = reader.byte()?;
                remaining = usize::from(more);
                extended = more == 255;
            }
        }
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLACK: Pixel = [0; 3];

    fn changed(row: &[Pixel], seed: &[Pixel]) -> Vec<usize> {
        (0..row.len()).filter(|&x| row[x] != seed[x]).collect()
    }

    #[test]
    fn empty_row_repeats_the_seed() {
        let seed = vec![[1, 2, 3]; 10];
        assert_eq!(decode_row(&[], &seed).unwrap(), seed);
    }

    #[test]
    fn decodes_first_row_of_the_captured_job() {
        // Row captured from the HP Deskjet F4400 driver.
        let data = [0x9F, 0xFF, 0xFF, 0x83, 0x00, 0x00, 0x00, 0x7B, 0xFF, 0xFF, 0xFF, 0x5B, 0x0C];
        let seed = vec![WHITE; 4800];
        let row = decode_row(&data, &seed).unwrap();
        let expected: Vec<usize> = (644..776).chain(1380..1401).collect();
        assert_eq!(changed(&row, &seed), expected);
        assert!(expected.iter().all(|&x| row[x] == BLACK));
    }

    #[test]
    fn literal_single_pixels_use_new_then_cached_colour() {
        // Second non-empty row of the captured job: one black pixel, then one cached.
        let data = [0x18, 0xFF, 0xFF, 0xFF, 0xFF, 0x2C, 0x00, 0x00, 0x00, 0x78, 0xFF, 0xFF, 0xFF, 0xC1];
        let seed = vec![WHITE; 4800];
        let row = decode_row(&data, &seed).unwrap();
        assert_eq!(changed(&row, &seed), vec![1067, 2029]);
        assert_eq!(row[2029], BLACK);
    }

    #[test]
    fn full_pixel_restores_white_and_keeps_colour() {
        // Literal run of 2: west source is unused here, both pixels written in full.
        let red_halved = [0x7F, 0x80, 0x00]; // (FF 00 00) >> 1
        let data = [&[0x01u8][..], &red_halved, &[0x7F, 0xFF, 0xFF]].concat();
        let seed = vec![BLACK; 4];
        let row = decode_row(&data, &seed).unwrap();
        assert_eq!(&row[..2], &[[255, 0, 0], WHITE]);
    }

    #[test]
    fn short_delta_is_relative_to_pixel_above() {
        // 0x8000 | dR=+1 | dG=-1 | dB=+2 (stored halved).
        let delta: u16 = 0x8000 | 1 << 10 | 0x1F << 5 | 1;
        let data = [0x00, (delta >> 8) as u8, delta as u8];
        let seed = vec![[100, 100, 100]; 2];
        assert_eq!(decode_row(&data, &seed).unwrap()[0], [101, 99, 102]);
    }

    #[test]
    fn truncated_or_overflowing_rows_are_errors() {
        let seed = vec![WHITE; 8];
        assert!(decode_row(&[0x00, 0x00], &seed).is_err());
        // Run of 9 on an 8-pixel row followed by another command.
        assert!(decode_row(&[0xE7, 0x00, 0xE0], &seed).is_err());
    }

    #[test]
    fn final_run_past_the_width_is_clipped() {
        let seed = vec![BLACK; 8];
        assert_eq!(decode_row(&[0xE7, 0x00], &seed).unwrap(), vec![WHITE; 8]);
    }
}
