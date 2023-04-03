#![allow(unstable_name_collisions)]
use harfbuzz_wasm::{debug, Font, Glyph, GlyphBuffer};
use itertools::Itertools;
use kurbo::{Affine, BezPath, ParamCurve, ParamCurveArclen, PathEl, Point};

use wasm_bindgen::prelude::*;

const SPACE_ID: u32 = 116;
const DOT_ID: u32 = 119;

fn dot_sequence(glyphs: Vec<Glyph>, buffer: &mut Vec<Glyph>, font: &Font, dot_spacing: f64) {
    if glyphs.is_empty() {
        return;
    }
    debug(&format!(
        "Dotting sequence: {}",
        glyphs
            .iter()
            .map(|x| font.get_glyph_name(x.codepoint))
            .intersperse("|".to_string())
            .collect::<String>()
    ));
    let mut lines: Vec<BezPath> = vec![];
    let mut this_line: Vec<PathEl> = vec![];
    let mut total_advance = 0;
    let mut dot_positions: Vec<Point> = vec![];
    for g in glyphs.iter() {
        for (ix, mut p) in font.get_outline(g.codepoint).into_iter().enumerate() {
            p.apply_affine(Affine::translate((total_advance as f64, 0.0)));
            let mut start_pt: Option<Point> = None;

            for el in p.elements().iter() {
                // debug(&format!("{}: {:?}", ix, el));

                match el {
                    PathEl::MoveTo(pt) => {
                        start_pt = Some(*pt);
                        // debug(&format!("Start point of seg is {}", pt));
                        if ix != 0 {
                            lines.push(BezPath::from_vec(this_line));
                            this_line = vec![];
                            continue;
                        }
                        this_line.push(PathEl::LineTo(*pt))
                    }
                    PathEl::LineTo(pt) => {
                        if pt.distance_squared(start_pt.unwrap()) > 20.0 {
                            this_line.push(*el)
                        }
                    }
                    PathEl::QuadTo(_, _) => this_line.push(*el),
                    PathEl::CurveTo(_, _, _) => this_line.push(*el),
                    PathEl::ClosePath => {} // this_line.push(*el),
                }
            }
        }
        total_advance += g.x_advance;
    }
    lines.push(BezPath::from_vec(this_line));

    // Now we have a set of lines, dot each line
    for line in lines {
        // Create a distance LUT for this line

        let mut distance_lut: Vec<(f64, Point)> = vec![];
        let mut total_length = 0_f64;
        for seg in line.segments() {
            let mut seg_distance = 0_f64;
            // debug(&format!("Seg: {:?}", seg));
            for t_int in 1..=100 {
                let t = t_int as f64 / 100.0;
                let split = seg.subsegment(0.0..t);
                let pt = seg.eval(t);
                seg_distance = split.arclen(0.1);
                // debug(&format!(
                //     "Length of seg 0..{} is {}",
                //     t,
                //     total_length + seg_distance
                // ));
                distance_lut.push((total_length + seg_distance, pt));
            }
            total_length += seg_distance;
        }
        // debug(&format!("Length of this line is {}", total_length));
        // debug(&format!("Number of dots is {}", total_length / dot_spacing));
        // debug(&format!("Lookup table is {:#?}", distance_lut));
        // Compute number of points on this line.
        let int_dots: usize = (total_length / dot_spacing) as usize;

        for i in 0..=int_dots {
            let pos: f64 = i as f64 / int_dots as f64 * total_length;
            let pt_ix = distance_lut.partition_point(|&(d, _pt)| d < pos);
            if let Some(el) = distance_lut.get(pt_ix) {
                let pt = el.1;
                if dot_positions
                    .iter()
                    .any(|pt2| pt2.distance(pt) < dot_spacing / 1.5)
                {
                    continue;
                }
                dot_positions.push(pt);
                buffer.push(Glyph {
                    codepoint: DOT_ID,
                    cluster: glyphs[0].cluster,
                    x_advance: 0,
                    y_advance: 0,
                    x_offset: pt.x as i32,
                    y_offset: pt.y as i32,
                    flags: 0,
                })
            }
        }
    }
    buffer.push(Glyph {
        codepoint: SPACE_ID,
        cluster: glyphs[0].cluster,
        x_advance: total_advance,
        y_advance: 0,
        x_offset: 0,
        y_offset: 0,
        flags: 0,
    })
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
    // let mut paths = font.get_outline(glyph);
    let (x_scale, _y_scale) = font.get_scale();
    let upem = face.get_upem();
    let scale_factor: f32 = x_scale as f32 / upem as f32;
    let mut buffer = GlyphBuffer::from_ref(buf_ref);
    let old_buffer = std::mem::take(&mut buffer.glyphs);
    let mut cur_sequence: Vec<Glyph> = vec![];

    let mut dot_width = font.get_glyph_extents(DOT_ID).width as f32 * 1.5 / scale_factor;

    if let Some(dtsp) = font.get_var_coords().get(1) {
        dot_width += *dtsp as f32 * 50.0 / scale_factor;
    }

    for (ix, item) in old_buffer.iter().enumerate() {
        let item_next = old_buffer
            .get(ix + 1)
            .map(|g| font.get_glyph_name(g.codepoint))
            .unwrap_or_else(|| "".to_string());
        let item_name = font.get_glyph_name(item.codepoint);
        debug(&format!("Current: {} next: {}", item_name, item_next));
        if item_name.contains('.') || item_next.contains('.') {
            cur_sequence.push(*item);
        } else {
            // Add self and resolve
            cur_sequence.push(*item);
            dot_sequence(
                cur_sequence,
                &mut buffer.glyphs,
                &font,
                (dot_width * scale_factor).into(),
            );
            cur_sequence = vec![];
        }
    }
    dot_sequence(
        cur_sequence,
        &mut buffer.glyphs,
        &font,
        (dot_width * scale_factor).into(),
    );

    1
}
