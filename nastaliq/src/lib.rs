mod dist;
mod glyph;
mod mindist;
mod outline;

// Auto-kerning routine, look in dist.rs for this.
use dist::{Kerner, Placed};
// Routines for interfacing with Harfbuzz
use harfbuzz_wasm::Font;
// With the Harfbuzz interface, we can choose how we want
// to represent a glyph. Here we use our own custom glyph
// representation so we can do clever things with it.
use glyph::{Collider, GlyphShape, GulzarBuffer};

use wasm_bindgen::prelude::*;

// In a serious Nastaliq shaper these values would be read
// from the font.
const KERN_DISTANCE: f32 = 300.0;
const BARI_YE_DOT_POSITION: f32 = -150.0;
const DOT_AVOIDANCE_DELTA: f32 = 50.0; // How much to move a colliding dot. Affects rendering speed.

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
// for later. The names and the paths are the same for every glyph
// with the same glyph ID, so we keep them once for each glyph ID,
// in a list of shapes that the glyphs refer to. (A shape gets the
// name here, and the paths when they are first wanted: look in
// glyph.rs and outline.rs.)
fn prepare_buffer(buffer: &mut GulzarBuffer, font: &Font) -> Vec<GlyphShape> {
    let mut shapes: Vec<GlyphShape> = vec![];
    // We look every glyph of the buffer up by its glyph ID, so the
    // looking up had better be quick: a little hash table of the glyph
    // IDs we have seen. A slot has a glyph ID and the number of its
    // shape, counting from one, with zero for a slot nobody has. There
    // are at least twice as many slots as glyphs, and their number is
    // a power of two.
    let mut seen: Vec<(u32, usize)> = vec![(0, 0); (buffer.glyphs.len() * 2).next_power_of_two()];
    for item in buffer.glyphs.iter_mut() {
        let mut slot = ((item.codepoint as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize;
        item.shape = loop {
            slot &= seen.len() - 1;
            if seen[slot].1 == 0 {
                shapes.push(GlyphShape::new(
                    item.codepoint,
                    &font.get_glyph_name(item.codepoint),
                ));
                seen[slot] = (item.codepoint, shapes.len());
            }
            if seen[slot].0 == item.codepoint {
                break seen[slot].1 - 1;
            }
            slot += 1;
        };
        item.kinds = shapes[item.shape].kinds;
    }
    set_total_advance(buffer);
    shapes
}

// The paths of a glyph of the buffer and where the glyph is, i.e.
// including the full running-total x advance, for the kerning routine.
fn placed<'a>(
    ix: usize,
    item: &glyph::GulzarGlyph,
    shapes: &'a [GlyphShape],
    font: &Font,
) -> Placed<'a> {
    Placed {
        key: ix,
        paths: shapes[item.shape].outline(font),
        dx: (item.x_total_advance + item.x_offset) as f64,
        dy: item.y_offset as f64,
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
    let shapes = prepare_buffer(&mut buffer, &font);

    // In this section we just mark all the glyphs which fall
    // above the tail of the bari ye.
    for mut item in buffer.glyphs.iter_mut() {
        if item.is_bari_ye() {
            // OK, we saw a bari ye. Let's first see how long the
            // tail was in units. We're going to then count off
            // each glyph's advance width so we know whether
            // of units we have left within the span of this glyph.
            let bari_ye_tail: i32 = shapes[item.shape].extents(&font).width - item.x_advance - 20;
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
    let mut kerner = Kerner::new();

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
            let mut left_glyphs = [placed(ix, this_item, &shapes, &font); 3];
            let other_glyph = placed(to_kern_with, &buffer.glyphs[to_kern_with], &shapes, &font);
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
                    let next_ix = ix3;
                    ix3 -= 1;
                    // Ignore dots for the purposes of kerning.
                    // We deal with them later.
                    if next.is_dot_below() || next.is_dot_above() {
                        continue;
                    }
                    counter += 1;
                    left_glyphs[counter] = placed(next_ix, next, &shapes, &font);
                } else {
                    break;
                }
            }
            let left_glyphs = &left_glyphs[..=counter];
            if left_glyphs.iter().all(|glyph| glyph.paths.is_empty())
                || other_glyph.paths.is_empty()
            {
                continue;
            }

            // OK, we found everything we want. Work out the
            // kern. If we saw a space, loosen things a little.
            let kern_required = kerner.determine_kern(
                left_glyphs,
                &other_glyph,
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

    // From here on we look for glyphs that collide.
    let mut collider = Collider::new(&buffer.glyphs, &shapes, &font, x_scale);

    // Drop dots within bari ye.
    let mut last_bari_ye_ix = 0;
    let mut bari_ye_dots = vec![];
    for (ix, item) in buffer.glyphs.iter().enumerate() {
        // If it's a bari ye, remember where it was.
        if item.is_bari_ye() {
            last_bari_ye_ix = ix;
        } else if item.in_bari_ye  // it's in the bari ye (we computed this earlier)
            && item.is_dot_below() // and it's a dot which collides
            && collider.collides(&buffer.glyphs, ix, last_bari_ye_ix)
        {
            bari_ye_dots.push(ix); // keep a note of it.
        }
    }

    // For each dot that falls within a bari ye and also collides,
    // set its absolute Y offset. You can't do *that* in OpenType!
    for ix in bari_ye_dots {
        let mut item = &mut buffer.glyphs[ix];
        let extents = shapes[item.shape].extents(&font);
        item.y_offset = extents.height - (BARI_YE_DOT_POSITION * scale_factor) as i32;
        collider.moved(ix, item);
    }

    // Let's do dot avoidance. Part one, dots below.
    // Each time we move a dot we have to check again, but not the
    // whole buffer: the dots from `checked` on were fine the last time
    // we looked, and nothing near them has moved since.
    let mut checked = buffer_len;
    loop {
        let mut to_lower: Option<usize> = None;
        // Walk backwards along the buffer, finding dots below
        for i in (0..checked).rev() {
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
                if !collider.collides(&buffer.glyphs, i, j) {
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
        collider.moved(to_lower, &buffer.glyphs[to_lower]);
        // A dot looks at the eight glyphs before it, so the dots which
        // could now collide with the one we moved are the eight after
        // it at most. Those after that are still fine.
        checked = (to_lower + 9).min(buffer_len);
    }

    // And this is basically the same thing but looking at
    // dots above.
    let mut checked = buffer_len;
    loop {
        let mut to_raise: Option<usize> = None;
        for i in (0..checked).rev() {
            if !buffer.glyphs[i].is_dot_above() {
                continue;
            }
            for j in ((i as i32 - 6).max(0) as usize)..(i + 6).min(buffer_len - 1) {
                if i == j {
                    continue;
                }
                if !collider.collides(&buffer.glyphs, i, j) {
                    continue;
                }
                to_raise = Some(i);
                break;
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
        collider.moved(to_raise, &buffer.glyphs[to_raise]);
        // Here a dot looks at the six glyphs before it.
        checked = (to_raise + 7).min(buffer_len);
    }

    // And we are done. Hand the buffer back to harfbuzz.

    1
}
