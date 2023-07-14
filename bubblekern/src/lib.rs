#![allow(unstable_name_collisions)]
mod dist;
mod glyph;

use dist::_determine_kern;
use glyph::BubbleBuffer;
use harfbuzz_wasm::{debug, Font};
use std::collections::BTreeMap;

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
    font.shape_with(buf_ref, "ot");
    let face = font.get_face();
    let (x_scale, _y_scale) = font.get_scale();
    let upem = face.get_upem();
    let scale_factor: f32 = x_scale as f32 / upem as f32;
    let mut buffer = BubbleBuffer::from_ref(buf_ref);
    let names_map: BTreeMap<String, u32> = (0..60_u32)
        .map(|id| (font.get_glyph_name(id), id))
        .collect();
    // Fill in bubble paths and total advance
    let mut total_advance = 0;
    for item in buffer.glyphs.iter_mut() {
        item.x_total_advance = total_advance;
        total_advance += item.x_advance;

        let this_name = font.get_glyph_name(item.codepoint);
        if let Some(&bubble_id) = names_map.get(&(this_name + ".bubble")) {
            item.bubble_paths = Some(font.get_outline(bubble_id))
        }
    }

    // Do the kern
    for ix in 0..(buffer.glyphs.len() - 1) {
        if let Some(left_paths) = buffer.glyphs[ix].positioned_bubble_paths(0.0) {
            // We push the right-hand glyphs outwards slightly, simply because the
            // algorithm does not do well if the bubbles start by overlapping.
            if let Some(right_paths) =
                buffer.glyphs[ix + 1].positioned_bubble_paths(200.0 * scale_factor)
            {
                let kern = 200.0 * scale_factor
                    + _determine_kern(&left_paths, &right_paths, 0.0 * scale_factor, scale_factor);
                debug(&format!("Kerning by {} at {}", kern, ix));
                buffer.glyphs[ix].x_advance += kern as i32;
            }
        }
    }
    1
}
