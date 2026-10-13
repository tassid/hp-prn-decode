//! Reads a captured HP PCL3GUI job and writes the page it would print as a PNG.

mod mode10;

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::{env, fs, process};

use mode10::{decode_row, Pixel, WHITE};

const ESC: u8 = 0x1B;

#[derive(Default)]
struct Page {
    width: usize,
    rows: Vec<Vec<Pixel>>,
    seed: Vec<Pixel>,
    data_rows: usize,
    blank_rows: usize,
    black_plane_bytes: usize,
    failed_rows: Vec<(usize, String)>,
    other_commands: BTreeMap<String, usize>,
}

impl Page {
    fn command(&mut self, name: [u8; 3], value: usize, payload: &[u8]) {
        match &name {
            b"*rS" => {
                self.width = value;
                self.seed = vec![WHITE; value];
            }
            // Vertical skip: blank rows, and the next row starts from white again.
            b"*bY" => {
                self.seed = vec![WHITE; self.width];
                self.blank_rows += value;
                self.rows.extend(std::iter::repeat(self.seed.clone()).take(value));
            }
            // Black plane of the current row. This decoder only draws the colour plane.
            b"*bV" => self.black_plane_bytes += payload.len(),
            b"*bW" if self.width > 0 => {
                match decode_row(payload, &self.seed) {
                    Ok(row) => self.seed = row,
                    Err(error) => {
                        let bytes: Vec<String> = payload.iter().map(|b| format!("{b:02X}")).collect();
                        self.failed_rows.push((self.rows.len(), format!("{error} [{}]", bytes.join(" "))));
                    }
                }
                self.data_rows += usize::from(!payload.is_empty());
                self.rows.push(self.seed.clone());
            }
            _ => *self.other_commands.entry(String::from_utf8_lossy(&name).into_owned()).or_default() += 1,
        }
    }
}

/// Walks the PCL escape sequences. `ESC * b 0v 13W<13 bytes>` is one sequence
/// carrying two commands: lowercase terminators chain, uppercase ends it.
fn parse(job: &[u8]) -> Page {
    let mut page = Page::default();
    let mut i = 0;
    while i < job.len() {
        if job[i] != ESC {
            i += 1;
            continue;
        }
        let parameterized = matches!(job.get(i + 1), Some(0x21..=0x2F)) && matches!(job.get(i + 2), Some(0x60..=0x7E));
        if !parameterized {
            i += 2;
            continue;
        }
        let (kind, group) = (job[i + 1], job[i + 2]);
        i += 3;
        loop {
            let digits = i;
            while matches!(job.get(i), Some(b'0'..=b'9' | b'+' | b'-' | b'.')) {
                i += 1;
            }
            let value = String::from_utf8_lossy(&job[digits..i]).parse::<f64>().unwrap_or(0.0).max(0.0) as usize;
            let Some(&terminator @ 0x40..=0x7E) = job.get(i) else { break };
            i += 1;
            let letter = terminator.to_ascii_uppercase();
            let carries_data = letter == b'W' || ([kind, group, letter] == *b"*bV");
            let end = if carries_data { (i + value).min(job.len()) } else { i };
            page.command([kind, group, letter], value, &job[i..end]);
            i = end;
            if terminator.is_ascii_uppercase() || terminator == b'@' {
                break;
            }
        }
    }
    page
}

fn write_png(page: &Page, path: &str) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|e| format!("não consegui criar {path}: {e}"))?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), page.width as u32, page.rows.len() as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    let pixels: Vec<u8> = page.rows.iter().flatten().flatten().copied().collect();
    writer.write_image_data(&pixels).map_err(|e| e.to_string())
}

fn run(input: &str, output: &str) -> Result<(), String> {
    let job = fs::read(input).map_err(|e| format!("não consegui ler {input}: {e}"))?;
    let page = parse(&job);
    if page.width == 0 || page.rows.is_empty() {
        return Err("não achei raster PCL3GUI nesse arquivo".to_string());
    }
    write_png(&page, output)?;

    println!("{} x {} px -> {output}", page.width, page.rows.len());
    println!("linhas com dados: {}", page.data_rows);
    println!("linhas iguais à de cima: {}", page.rows.len() - page.blank_rows - page.data_rows);
    println!("linhas puladas em branco: {}", page.blank_rows);
    println!("bytes no plano preto (não desenhados): {}", page.black_plane_bytes);
    println!("outros comandos: {:?}", page.other_commands);
    for (row, error) in page.failed_rows.iter().take(10) {
        println!("linha {row} falhou: {error}");
    }
    if page.failed_rows.is_empty() {
        Ok(())
    } else {
        Err(format!("{} linha(s) não decodificaram", page.failed_rows.len()))
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let [_, input, output] = args.as_slice() else {
        eprintln!("uso: hp-prn-decode <job.prn> <saida.png>");
        process::exit(2);
    };
    if let Err(error) = run(input, output) {
        eprintln!("erro: {error}");
        process::exit(1);
    }
}
