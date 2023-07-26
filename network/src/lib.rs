use harfbuzz_wasm::{debug, Font, Glyph, GlyphBuffer};
use tiny_rng::{Rand, Rng};
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
    let mut newglyphs = vec![];
    let (x_scale, _y_scale) = font.get_scale();
    let face = font.get_face();
    let upem = face.get_upem();
    let scale_factor: f32 = x_scale as f32 / upem as f32;
    let mut rng = Rng::from_seed(
        buffer
            .glyphs
            .iter()
            .map(|i| i.codepoint)
            .sum::<u32>()
            .into(),
    );
    let rand1 = rng.rand_u8();
    let rand2 = rng.rand_u8();

    let step = (30.0 * scale_factor) as i32;
    // Get buffer as string
    for item in &buffer.glyphs {
        let glyph_id = font.get_glyph(item.codepoint, 0);
        newglyphs.push(Glyph {
            codepoint: glyph_id,
            x_advance: 0,
            y_advance: 0,
            x_offset: 0,
            y_offset: 0,
            cluster: item.cluster,
            flags: 0,
        });
        let rand1 = rng.rand_range_i32(-2, 2);
        let rand2 = rng.rand_range_i32(-2, 2);
        newglyphs.push(Glyph {
            codepoint: glyph_id,
            x_advance: 0,
            y_advance: 0,
            x_offset: rand1 * step,
            y_offset: rand2 * step,
            cluster: item.cluster,
            flags: 0,
        });
        let rand1 = rng.rand_range_i32(-2, 2);
        let rand2 = rng.rand_range_i32(-2, 2);
        newglyphs.push(Glyph {
            codepoint: glyph_id,
            x_advance: font.get_glyph_h_advance(glyph_id),
            y_advance: 0,
            x_offset: rand1 * step,
            y_offset: rand2 * step,

            cluster: item.cluster,
            flags: 0,
        });
    }
    buffer.glyphs = newglyphs;
    // Buffer is written back to HB on drop
    1
}
