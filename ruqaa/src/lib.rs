mod dist;
mod glyph;

use dist::_determine_kern;
use glyph::GulzarBuffer;
use harfbuzz_wasm::{debug, Font};
use kurbo::{Affine, BezPath, Shape};
use std::collections::BTreeMap;

use wasm_bindgen::prelude::*;

fn get_scaled_outline(font: &Font, glyph: u32) -> Vec<BezPath> {
    let mut paths = font.get_outline(glyph);
    if let Some(big_bounds) = paths
        .iter()
        .map(|x| x.bounding_box())
        .reduce(|a, b| a.union(b))
    {
        let center_vec = big_bounds.center().to_vec2();
        let affine = Affine::translate(center_vec * -1.0)
            * Affine::scale(1.1)
            * Affine::translate(center_vec);
        for path in paths.iter_mut() {
            path.apply_affine(affine);
        }
    }
    paths
}

fn set_total_advance(buffer: &mut GulzarBuffer) {
    let mut total_advance = 0;
    for mut item in buffer.glyphs.iter_mut() {
        // Fill total advances
        item.x_total_advance = total_advance;
        total_advance += item.x_advance;
    }
}
fn prepare_buffer(buffer: &mut GulzarBuffer, font: &Font) {
    let mut cache: BTreeMap<u32, Vec<BezPath>> = BTreeMap::new();
    for mut item in buffer.glyphs.iter_mut() {
        item.name = font.get_glyph_name(item.codepoint);
    }
    for mut item in buffer.glyphs.iter_mut() {
        item.paths = cache
            .entry(item.codepoint)
            .or_insert_with(|| get_scaled_outline(font, item.codepoint))
            .clone();
    }
    set_total_advance(buffer)
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
    font.shape_with(buf_ref, "ot");
    let face = font.get_face();
    let (x_scale, _y_scale) = font.get_scale();
    let upem = face.get_upem();
    let scale_factor: f32 = x_scale as f32 / upem as f32;
    // debug(&format!("Scale factor: {:}", scale_factor));

    let mut buffer = GulzarBuffer::from_ref(buf_ref);
    prepare_buffer(&mut buffer, &font);

    // Kerning
    let buffer_len = buffer.glyphs.len();
    let mut kerns = vec![];
    for ix in 0..buffer_len {
        let this_item = &buffer.glyphs[ix];
        let mut ix2 = ix + 1;
        let mut to_kern_with = None;
        if !(this_item.name.contains(".init")
            || (this_item.name.ends_with("-ar") && !this_item.is_dot()))
        {
            continue;
        }
        while ix2 < buffer_len {
            if buffer.glyphs[ix2].name.contains("space") && buffer.glyphs[ix2].x_advance > 0 {
                break;
            }

            if (buffer.glyphs[ix2].name.ends_with("-ar") && !buffer.glyphs[ix2].is_dot())
                || buffer.glyphs[ix2].name.contains("fina")
            {
                to_kern_with = Some(ix2);
                break;
            }
            ix2 += 1;
            continue;
        }
        if let Some(to_kern_with) = to_kern_with {
            let other_item = &buffer.glyphs[to_kern_with];
            let kern_required = _determine_kern(
                &this_item.positioned_paths(),
                &other_item.positioned_paths(),
                100.0 * scale_factor,
                0.0,
                scale_factor,
            );
            debug(&format!(
                "Kern between {} and {}: {}",
                this_item.name, other_item.name, kern_required,
            ));
            kerns.push((ix, kern_required as i32));
        }
    }

    for (ix, kern_required) in kerns {
        buffer.glyphs[ix].x_advance += kern_required;
    }

    set_total_advance(&mut buffer);

    // Vertical positioning
    let mut start_of_word = buffer_len - 1;
    let mut words = vec![];
    while start_of_word > 0 {
        let this_item = &buffer.glyphs[start_of_word];
        if !this_item.name.contains(".init") {
            start_of_word -= 1;
            continue;
        }
        debug(&format!(
            "Word start {:}: {:} @{:}",
            start_of_word, this_item.name, this_item.y_offset
        ));
        let mut ix = start_of_word;
        while !buffer.glyphs[ix].name.contains(".fina") {
            ix -= 1;
        }
        debug(&format!(
            "Word end {:}: {:} @{:}",
            ix, buffer.glyphs[ix].name, buffer.glyphs[ix].y_offset
        ));

        words.push((
            start_of_word,
            ix,
            -(buffer.glyphs[start_of_word].y_offset) / 2,
        ));
        start_of_word = ix;
    }

    for (start, end, shift) in words {
        debug(&format!(
            "Vertically shifting between {} and {} by {}",
            start, end, shift
        ));
        for ix in end..=start {
            buffer.glyphs[ix].y_offset += shift;
        }
    }
    1
}
