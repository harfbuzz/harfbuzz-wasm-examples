use harfbuzz_wasm::{debug, Font, Glyph, GlyphBuffer};
use regex::Regex;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn shape(
    _shape_plan: u32,
    font_ref: u32,
    buf_ref: u32,
    _features: u32,
    _num_features: u32,
) -> i32 {
    let font = Font::from_ref(font_ref);
    let mut buffer = GlyphBuffer::from_ref(buf_ref);
    // Get buffer as string
    let buf_u8: Vec<u8> = buffer.glyphs.iter().map(|g| g.codepoint as u8).collect();
    let str_buf = String::from_utf8_lossy(&buf_u8);

    let re = Regex::new(r"(\d+(?:\.\d+)?)\s*([\+-/\*])\s*(\d+(?:\.\d+)?)\s*=").unwrap();
    if let Some(captures) = re.captures(&str_buf) {
        debug(&format!("{:?}", captures));
        let left = captures[1].parse::<f32>().unwrap();
        let right = captures[3].parse::<f32>().unwrap();
        let res = match &captures[2] {
            "+" => left + right,
            "-" => left - right,
            "*" => left * right,
            "/" => left / right,
            _ => panic!(),
        };
        let res_str = format!("{}", res);
        buffer.glyphs = res_str
            .chars()
            .enumerate()
            .map(|(ix, x)| Glyph {
                codepoint: x as u32,
                flags: 0,
                x_advance: 0,
                y_advance: 0,
                cluster: ix as u32,
                x_offset: 0,
                y_offset: 0,
            })
            .collect();
    } else {
        debug("No match");
        debug(&str_buf);
    }

    for mut item in buffer.glyphs.iter_mut() {
        // Map character to glyph
        item.codepoint = font.get_glyph(item.codepoint, 0);
        // Set advance width
        item.x_advance = font.get_glyph_h_advance(item.codepoint);
    }
    // Buffer is written back to HB on drop
    1
}
