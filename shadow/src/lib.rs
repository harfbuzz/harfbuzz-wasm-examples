use harfbuzz_wasm::{Font, GlyphBuffer};

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
    let mut total_width = 0;

    for mut item in buffer.glyphs.iter_mut() {
        // Map character to glyph
        item.codepoint = font.get_glyph(item.codepoint, 0);
        // Set advance width
        item.x_advance = font.get_glyph_h_advance(item.codepoint);
        total_width += item.x_advance;
    }

    let mut cloned = buffer.glyphs.clone();
    buffer.glyphs.last_mut().unwrap().x_advance -= total_width;
    for item in buffer.glyphs.iter_mut() {
        item.codepoint += 26;
    }
    for item in cloned.iter_mut() {
        item.cluster += buffer.glyphs.len() as u32;
    }
    buffer.glyphs.extend(cloned);

    // Buffer is written back to HB on drop
    1
}
