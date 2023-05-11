// This is going to be an example of embedding a rasterizer
// inside a font, using the outlines of an "inner font" to
// determine where the pixels are going to go. It's a scary
// idea, but please trust me, the code is not too complicated
// to follow! Come on, it's less than 200 lines.

// First, we're going to need our rasterizing library.
use ab_glyph_rasterizer::Rasterizer;
// harfbuzz_wasm provides access to structures related to Harfbuzz
// shaping.
use harfbuzz_wasm::{Buffer, CGlyphExtents, Font, Glyph, GlyphBuffer};
// And kurbo is a library which helps manipulate curve structures.
use kurbo::{
    Affine, BezPath,
    PathSeg::{Cubic, Line, Quad},
};
// Finally this will give us the connection to the WASM engine.
use wasm_bindgen::prelude::*;

// Get the bezier paths of a glyph, by ID, and apply a scaling transform.
fn get_scaled_outline(font: &Font, glyph: u32, scale_factor: f64) -> Vec<BezPath> {
    let mut paths = font.get_outline(glyph);
    let affine = Affine::scale(scale_factor);
    for path in paths.iter_mut() {
        path.apply_affine(affine);
    }
    paths
}

// Silly little utility function to convert between the point
// representations in kurbo and the point representations in the
// rasterizer.
fn p(p: kurbo::Point) -> ab_glyph_rasterizer::Point {
    ab_glyph_rasterizer::point(p.x as f32, p.y as f32)
}

// This is the rasterizer, which turns curves coming from the
// "inner font" into a series of pixel components and positions
// them.
fn rasterize(
    glyph_metrics: CGlyphExtents,
    paths: &[BezPath],
    cluster: u32,
    pixel_size: f32,
) -> Vec<Glyph> {
    // Our pixel grid will have the same *unit* height and width
    // as the glyph we're rasterizing, but we divide by the pixel
    // height to work out the number of pixels in the grid.
    let width = (((glyph_metrics.x_bearing + glyph_metrics.width) as f32 + 0.5) / pixel_size).ceil()
        as usize;
    let height = (-glyph_metrics.height as f32 / pixel_size).ceil() as usize;

    // We create a new rasterizer; all this is handled in the
    // ab_glyph_rasterizer library.
    let mut rasterizer = Rasterizer::new(width, height);
    // And this is where we will put our list of positioned pixel
    // glyphs.
    let mut glyphs = vec![];

    // Draw the paths onto the rasterizer.
    for path in paths {
        for seg in path.segments() {
            match seg {
                Line(l) => rasterizer.draw_line(p(l.p0), p(l.p1)),
                Quad(q) => rasterizer.draw_quad(p(q.p0), p(q.p1), p(q.p2)),
                Cubic(c) => rasterizer.draw_cubic(p(c.p0), p(c.p1), p(c.p2), p(c.p3)),
            }
        }
    }
    // Now let's look at all the results of rasterization.
    rasterizer.for_each_pixel_2d(|x, y, alpha| {
        // If there's no pixel here, move on.
        if alpha < 0.1 {
            return;
        }
        // Choose the appropriate color font glyph (white,
        // light grey, dark grey) based on the coverage of
        // this pixel.
        let color = if alpha > 0.5 {
            2
        } else if alpha > 0.3 {
            3
        } else {
            4
        };
        // Create an output glyph representing this pixel, at
        // this color and this position.
        glyphs.push(Glyph {
            cluster,
            codepoint: color,
            x_advance: 0,
            y_advance: 0,
            flags: 0,
            x_offset: ((x as f32) * pixel_size) as i32,
            y_offset: ((y as f32) * pixel_size) as i32,
        });
    });
    glyphs
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
    let face = font.get_face();

    // There's a table inside the font called "Font", so read
    // that, and turn it into a Harfbuzz font structure.
    let inner_font_blob = face.reference_table("Font");
    let inner_face = inner_font_blob.into_face(0);
    let inner_font = inner_face.create_font();

    // Find the size of the pixel. This will already have been
    // affected by variation on the opsz axis.
    let pixel_size = -font.get_glyph_extents(2).height as f32;

    // Take all the other variation axis settings and apply
    // them to the inner font.
    let coords = font.get_var_coords();
    if coords.len() > 1 {
        inner_font.set_var_coords(&coords[2..]);
    }

    // Run ordinary OpenType shaping on the inner font, using
    // the current buffer, and get the result.
    inner_font.shape_with(buf_ref, "ot");
    let mut buffer: GlyphBuffer = Buffer::from_ref(buf_ref);

    let mut new_glyphs: Vec<Glyph> = vec![];
    for item in buffer.glyphs.iter_mut() {
        // Find out how big the "inner" glyph would be.
        let glyph_metrics = inner_font.get_glyph_extents(item.codepoint);
        // Scale the paths based on the optical size.
        let paths = get_scaled_outline(&inner_font, item.codepoint, (1.0 / pixel_size).into());
        // Rasterize the inner glyph and add the glyphs represent the pixels to the output buffer
        new_glyphs.extend(rasterize(glyph_metrics, &paths, item.cluster, pixel_size));
        // The pixels were marks and had no space of their own, so we
        // need to advance the cursor using an empty space glyph with
        // the width of the scaled inner glyph.
        let x_advance = (item.x_advance * face.get_upem() as i32) / (inner_face.get_upem() as i32);
        new_glyphs.push(Glyph {
            cluster: item.cluster,
            codepoint: 1, // space,
            x_advance,
            y_advance: 0,
            flags: 0,
            x_offset: 0,
            y_offset: 0,
        });
    }
    // Send our new buffer back to the shaping engine.
    buffer.glyphs = new_glyphs;
    1
}
