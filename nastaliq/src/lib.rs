mod dist;
mod glyph;

// Auto-kerning routine, look in dist.rs for this.
use dist::determine_kern;
// Routines for interfacing with Harfbuzz
use harfbuzz_wasm::{debug, Font};
// With the Harfbuzz interface, we can choose how we want
// to represent a glyph. Here we use our own custom glyph
// representation so we can do clever things with it.
use glyph::GulzarBuffer;
// Kurbo is a library for doing mathematics on bezier curves.
use kurbo::{Affine, BezPath, Shape};
use std::collections::BTreeMap;

use wasm_bindgen::prelude::*;

// In a serious Nastaliq shaper these values would be read
// from the font.
const KERN_DISTANCE: f32 = 300.0;
const BARI_YE_DOT_POSITION: f32 = -150.0;
const DOT_AVOIDANCE_DELTA: f32 = 50.0; // How much to move a colliding dot. Affects rendering speed.

// Return a slightly scaled-up copy of a glyph's outline.
// We create this slightly bigger copy of the glyphs so that
// when we do collision tests between glyphs, they have a bit
// of breathing space around them.
fn get_scaled_outline(font: &Font, glyph: u32) -> Vec<BezPath> {
    let mut paths = font.get_outline(glyph);
    // Find a bounding box which encompasses all the paths by
    // taking the union of all of their bounding boxes.
    if let Some(big_bounds) = paths
        .iter()
        .map(|x| x.bounding_box())
        .reduce(|a, b| a.union(b))
    {
        // Scale up 10% around the center.
        let center_vec = big_bounds.center().to_vec2();
        let affine = Affine::translate(center_vec * -1.0)
            * Affine::scale(1.12)
            * Affine::translate(center_vec);
        for path in paths.iter_mut() {
            path.apply_affine(affine);
        }
    }
    paths
}

// Normally Harfbuzz buffers give you the advance for each glyph
// but it turns out to be quite useful for us to keep a running
// total in the data structure representing each glyph.
// We compute this total x advance by summing the advances.
fn set_total_advance(buffer: &mut GulzarBuffer) {
    let mut total_advance = 0;
    for mut item in buffer.glyphs.iter_mut() {
        // Fill total advances
        item.x_total_advance = total_advance;
        total_advance += item.x_advance;
    }
}

// We want to know three things: the name of each glyph,
// their paths, and the running total advance, so this routine
// just gets that information ready in the buffer to help us
// for later.
fn prepare_buffer(buffer: &mut GulzarBuffer, font: &Font) {
    let mut cache: BTreeMap<u32, Vec<BezPath>> = BTreeMap::new();
    for mut item in buffer.glyphs.iter_mut() {
        item.name = font.get_glyph_name(item.codepoint);
    }
    for mut item in buffer.glyphs.iter_mut() {
        item.paths = cache
            .entry(item.codepoint)
            .or_insert_with(|| get_scaled_outline(font, item.codepoint))
            .clone(); // This clone is bad code, I know.
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
    // OK, this is the main shaping routine. First, get hold
    // of Harfbuzz's copy of the font and use OpenType shaping.
    // This just gives glyph selection, cursive attachment and
    // mark positioning. No kerning or collision mitigations yet.
    let font = Font::from_ref(font_ref);
    font.shape_with(buf_ref, "ot");

    let mut bari_ye_counter: Option<i32> = None;

    // Find out how big things are.
    let face = font.get_face();
    let (x_scale, _y_scale) = font.get_scale();
    let upem = face.get_upem();
    let scale_factor: f32 = x_scale as f32 / upem as f32;

    // Get the Buffer from Harfbuzz and fill in the information
    // we need.
    let mut buffer = GulzarBuffer::from_ref(buf_ref);
    prepare_buffer(&mut buffer, &font);

    // In this section we just mark all the glyphs which fall
    // above the tail of the bari ye.
    for mut item in buffer.glyphs.iter_mut() {
        if item.is_bari_ye() {
            // OK, we saw a bari ye. Let's first see how long the
            // tail was in units. We're going to then count off
            // each glyph's advance width so we know whether
            // of units we have left within the span of this glyph.
            let bari_ye_tail: i32 =
                font.get_glyph_extents(item.codepoint).width - item.x_advance - 20;
            bari_ye_counter = Some(bari_ye_tail);
            // and now we start marking glyphs as being above the
            // bari ye.
            item.in_bari_ye = true;
        } else if let Some(mut remainder) = bari_ye_counter {
            // Init glyphs stop the sequence, but if we have an
            // init glyph and we are still within the bari ye tail
            // (something like بے) we need to add some padding to
            // the init glyph so that we clear the tail. (Otherwise
            // in ابے the alif clashes with the bari ye.)
            if item.is_init() && remainder > 0 {
                debug(&format!(
                    "Adding {:} advance to {:} to fill bari-ye tail",
                    remainder, item.name
                ));
                item.x_advance += (remainder as f32 / scale_factor) as i32;
                remainder = 0;
            } else {
                // We're still in the sequence, so we work out
                // how many we have left.
                remainder -= item.x_advance;
            }
            // Are we still going, or are we out from the bari ye?
            if remainder > 0 {
                bari_ye_counter = Some(remainder);
                item.in_bari_ye = true;
            } else {
                bari_ye_counter = None;
            }
        }
    }

    // Okay, all glyphs affected by bari ye tail are marked.
    // Let's move on to kerning.

    let buffer_len = buffer.glyphs.len();
    let mut kerns = vec![];

    for ix in 0..buffer_len {
        let this_item = &buffer.glyphs[ix];
        let mut ix2 = ix + 1;
        let mut to_kern_with = None;
        let mut seen_space = false;
        // We only kern inits/isols against finas so skip everything
        // else.
        if !(this_item.is_init() || this_item.is_isol()) {
            continue;
        }
        // Find the second thing to kern, knowing that there might be
        // a space in the middle.
        while ix2 < buffer_len {
            if buffer.glyphs[ix2].is_space() && buffer.glyphs[ix2].x_advance > 0 {
                seen_space = true;
            }

            if buffer.glyphs[ix2].is_isol() || buffer.glyphs[ix2].is_fina() {
                // OK, we found it.
                to_kern_with = Some(ix2);
                break;
            }
            ix2 += 1;
            continue;
        }

        // Now we have a left glyph and a right glyph.
        if let Some(to_kern_with) = to_kern_with {
            let mut left_paths = this_item.positioned_paths();
            let other_paths = &buffer.glyphs[to_kern_with].positioned_paths();
            // We're actually going to extend those paths with
            // some more context on the left side,
            // to deal with things like بلی - the choti ye is part
            // of the lam stroke, and just comparing be/lam would be
            // bad; it would be too close and bump into the choti ye.
            let mut counter = 0;
            let mut ix3 = ix - 1;
            while counter < 2 {
                if ix3 == 0 {
                    break;
                }
                if let Some(next) = buffer.glyphs.get(ix3) {
                    ix3 -= 1;
                    // Ignore dots for the purposes of kerning.
                    // We deal with them later.
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

            // OK, we found everything we want. Work out the
            // kern. If we saw a space, loosen things a little.
            let kern_required = determine_kern(
                &left_paths,
                other_paths,
                KERN_DISTANCE * scale_factor,
                0.0,
                scale_factor,
            ) + if seen_space {
                480.0 * scale_factor
            } else {
                0.0
            };
            // debug(&format!(
            //     "Kern between {} and {}: {}",
            //     this_item.name, buffer.glyphs[to_kern_with].name, kern_required,
            // ));

            // Only tighten things, don't make them looser.
            if kern_required < 0.0 {
                kerns.push((ix, kern_required as i32));
            }
        }
    }

    // Now we apply the kerning.
    for (ix, kern_required) in kerns {
        buffer.glyphs[ix].x_advance += kern_required;
    }
    // We changed the advances so we should recompute.
    set_total_advance(&mut buffer);

    // Drop dots within bari ye.
    let mut last_bari_ye_ix = 0;
    let mut bari_ye_dots = vec![];
    for (ix, item) in buffer.glyphs.iter().enumerate() {
        // If it's a bari ye, remember where it was.
        if item.is_bari_ye() {
            last_bari_ye_ix = ix;
        } else if item.in_bari_ye  // it's in the bari ye (we computed this earlier)
            && item.is_dot_below() // and it's a dot which collides
            && item.collides(&buffer.glyphs[last_bari_ye_ix], &font)
        {
            bari_ye_dots.push(ix); // keep a note of it.
        }
    }

    // For each dot that falls within a bari ye and also collides,
    // set its absolute Y offset. You can't do *that* in OpenType!
    for ix in bari_ye_dots {
        let mut item = &mut buffer.glyphs[ix];
        let extents = font.get_glyph_extents(item.codepoint);
        item.y_offset = extents.height - (BARI_YE_DOT_POSITION * scale_factor) as i32;
    }

    // Let's do dot avoidance. Part one, dots below.
    loop {
        let mut to_lower: Option<usize> = None;
        // Walk backwards along the buffer, finding dots below
        for i in (0..buffer_len).rev() {
            if !buffer.glyphs[i].is_dot_below() {
                continue;
            }
            // Look at quite a wide context on both sides to see
            // if there are collisions.
            for j in ((i as i32 - 8).max(0) as usize)..(i + 8).min(buffer_len - 1) {
                // Obviously it's going to collide with itself, so ignore that...
                if i == j {
                    continue;
                }
                // If this dot doesn't collide with something, we're fine.
                if !buffer.glyphs[i].collides(&buffer.glyphs[j], &font) {
                    continue;
                }
                // If this dot collided with an later dot, lower
                // *that* one instead.
                if buffer.glyphs[j].is_dot_below() && j < i {
                    to_lower = Some(j);
                    break;
                } else {
                    // else lower this one.
                    to_lower = Some(i);
                    break;
                }
            }
            // If we find a dot to lower, let's go deal with
            // that now.
            if to_lower.is_some() {
                break;
            }
        }
        // If there are no dots left which collided, we're done.
        if to_lower.is_none() {
            break;
        }
        // Otherwise, fix this dot and go check again.
        let to_lower = to_lower.unwrap();
        buffer.glyphs[to_lower].y_offset -= (DOT_AVOIDANCE_DELTA * scale_factor) as i32;
    }

    // And this is basically the same thing but looking at
    // dots above.
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
        buffer.glyphs[to_raise].y_offset += (50.0 * scale_factor) as i32;
    }

    // And we are done. Hand the buffer back to harfbuzz.

    1
}
