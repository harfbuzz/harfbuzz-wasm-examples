mod dist;
mod glyph;

use dist::_determine_kern;
use glyph::GulzarBuffer;
use harfbuzz_wasm::{debug, Font};
use kurbo::{Affine, BezPath, Rect, Shape};
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
    let mut bari_ye_counter: Option<i32> = None;
    let (x_scale, _y_scale) = font.get_scale();
    let upem = face.get_upem();
    let scale_factor: f32 = x_scale as f32 / upem as f32;
    let bari_ye_tail: i32 = (633.0 * scale_factor) as i32;
    // debug(&format!("Scale factor: {:}", scale_factor));

    let mut buffer = GulzarBuffer::from_ref(buf_ref);
    prepare_buffer(&mut buffer, &font);

    for mut item in buffer.glyphs.iter_mut() {
        if item.name == "BARI_YEf1" {
            // OK, we saw a bari ye. Let's count down the number
            // of units we have left within the span of this glyph.
            bari_ye_counter = Some(bari_ye_tail + item.x_advance);
            item.in_bari_ye = true;
        } else if let Some(mut remainder) = bari_ye_counter {
            // Init glyphs stop the sequence
            if item.name.contains('i') && remainder > 0 {
                debug(&format!(
                    "Adding {:} advance to {:} to fill bari-ye tail",
                    remainder, item.name
                ));
                item.x_advance += remainder;
                remainder = 0;
            } else {
                remainder -= item.x_advance;
            }
            if remainder > 0 {
                bari_ye_counter = Some(remainder);
                item.in_bari_ye = true;
            } else {
                bari_ye_counter = None;
            }
        }
    }

    let buffer_len = buffer.glyphs.len();

    // Kerning
    let mut kerns = vec![];
    for ix in 0..buffer_len {
        let this_item = &buffer.glyphs[ix];
        let mut ix2 = ix + 1;
        let mut to_kern_with = None;
        if !(this_item.name.contains('i') || this_item.name.contains('u')) {
            continue;
        }
        while ix2 < buffer_len {
            if buffer.glyphs[ix2].name.contains("space") && buffer.glyphs[ix2].x_advance > 0 {
                break;
            }

            if buffer.glyphs[ix2].name.contains('u') || buffer.glyphs[ix2].name.contains('f') {
                to_kern_with = Some(ix2);
                break;
            }
            ix2 += 1;
            continue;
        }
        if let Some(to_kern_with) = to_kern_with {
            let mut left_paths = this_item.positioned_paths();
            let other_paths = &buffer.glyphs[to_kern_with].positioned_paths();
            // Grab a few more items
            let mut counter = 0;
            let mut ix3 = ix - 1;
            while counter < 2 {
                if ix3 == 0 {
                    break;
                }
                if let Some(next) = buffer.glyphs.get(ix3) {
                    ix3 -= 1;
                    if next.is_dot_below() || next.is_dot_above() {
                        continue;
                    }
                    left_paths.extend_from_slice(&next.positioned_paths());
                    counter += 1;
                } else {
                    break;
                }
            }
            if left_paths.is_empty() || other_paths.is_empty() {
                continue;
            }
            let kern_required = _determine_kern(
                &left_paths,
                other_paths,
                150.0 * scale_factor,
                0.0,
                scale_factor,
            );
            // debug(&format!(
            //     "Kern between {} and {}: {}",
            //     this_item.name, other_item.name, kern_required,
            // ));
            kerns.push((ix, kern_required as i32));
        }
    }

    for (ix, kern_required) in kerns {
        buffer.glyphs[ix].x_advance += kern_required;
    }

    set_total_advance(&mut buffer);

    // Drop dots, sometimes
    let mut last_bari_ye_ix = 0;
    let mut bari_ye_dots = vec![];
    for (ix, item) in buffer.glyphs.iter().enumerate() {
        if item.name == "BARI_YEf1" {
            last_bari_ye_ix = ix;
        }
        if item.in_bari_ye
            && item.is_dot_below()
            && item.collides(&buffer.glyphs[last_bari_ye_ix], &font)
        {
            bari_ye_dots.push(ix);
        }
    }
    for ix in bari_ye_dots {
        let mut item = &mut buffer.glyphs[ix];
        let extents = font.get_glyph_extents(item.codepoint);
        item.y_offset = extents.height - (50.0 * scale_factor) as i32;
    }

    // // Let's do dot avoidance
    // // below
    loop {
        let mut to_lower: Option<usize> = None;
        for i in (0..buffer_len).rev() {
            if !buffer.glyphs[i].is_dot_below() {
                continue;
            }
            for j in ((i as i32 - 8).max(0) as usize)..(i + 8).min(buffer_len - 1) {
                if i == j {
                    continue;
                }
                if !buffer.glyphs[i].collides(&buffer.glyphs[j], &font) {
                    continue;
                }
                if buffer.glyphs[j].is_dot_below() && j < i {
                    to_lower = Some(j);
                    break;
                } else {
                    to_lower = Some(i);
                    break;
                }
            }
            if to_lower.is_some() {
                break;
            }
        }
        if to_lower.is_none() {
            break;
        }
        let to_lower = to_lower.unwrap();
        buffer.glyphs[to_lower].y_offset -= (150.0 * scale_factor) as i32;
    }

    // // above
    loop {
        let mut to_raise: Option<usize> = None;
        for i in (0..buffer_len).rev() {
            if !buffer.glyphs[i].is_dot_above() {
                continue;
            }
            for j in ((i as i32 - 6).max(0) as usize)..(i + 6).min(buffer_len - 1) {
                if i == j {
                    continue;
                }
                if !buffer.glyphs[i].collides(&buffer.glyphs[j], &font) {
                    continue;
                }
                to_raise = Some(i);
            }
            if to_raise.is_some() {
                break;
            }
        }
        if to_raise.is_none() {
            break;
        }
        let to_raise = to_raise.unwrap();
        buffer.glyphs[to_raise].y_offset += (150.0 * scale_factor) as i32;
    }

    1
}
