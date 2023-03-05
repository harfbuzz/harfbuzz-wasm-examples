// We don't use the usual harfbuzz-wasm for this, because
// we are doing evil.
mod hbwasm;
use crate::hbwasm::{
    buffer_set_contents, Buffer, BufferItem, CBufferContents, CGlyphInfo, CGlyphPosition, Font,
    Glyph, GlyphBuffer,
};

use wasm_bindgen::prelude::*;

fn putback(buffer: &mut GlyphBuffer) {
    let mut positions: Vec<CGlyphPosition>;
    let mut infos: Vec<CGlyphInfo>;
    let glyphs = std::mem::take(&mut buffer.glyphs);
    (infos, positions) = glyphs.into_iter().map(|g| g.to_c()).unzip();
    let c_contents = CBufferContents {
        length: positions.len() as u32,
        info: infos[..].as_mut_ptr(),
        position: positions[..].as_mut_ptr(),
    };
    unsafe {
        buffer_set_contents(buffer._ptr, &c_contents);
    }
}

#[wasm_bindgen]
pub fn shape(
    _shape_plan: u32,
    font_ref: u32,
    buf_ref: u32,
    _features: u32,
    _num_features: u32,
) -> i32 {
    let font = Font::from_ref(font_ref);
    let mut buffer: GlyphBuffer = Buffer::from_ref(buf_ref);

    // Get buffer as string
    let buf_u8: Vec<u8> = buffer.glyphs.iter().map(|g| g.codepoint as u8).collect();
    let str_buf = String::from_utf8_lossy(&buf_u8);

    if !str_buf.starts_with("debug: ") {
        font.shape_with(buf_ref, "ot");
        std::mem::forget(buffer);
        return 1;
    }
    buffer.glyphs = buffer.glyphs[7..].to_vec();
    putback(&mut buffer);
    font.shape_with(buf_ref, "ot");
    std::mem::forget(buffer);

    let mut buffer = GlyphBuffer::from_ref(buf_ref);
    let names: String = buffer
        .glyphs
        .iter()
        .map(|g| font.get_glyph_name(g.codepoint))
        .collect::<Vec<String>>()
        .join("|");
    // debug(&format!("Buffer now is : {:?}", names));

    buffer.glyphs = names
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

    for mut item in buffer.glyphs.iter_mut() {
        // Map character to glyph
        item.codepoint = font.get_glyph(item.codepoint, 0);
        // Set advance width
        item.x_advance = font.get_glyph_h_advance(item.codepoint);
    }
    1
}
