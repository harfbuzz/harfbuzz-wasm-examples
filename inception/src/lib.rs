use ab_glyph_rasterizer::Rasterizer;
use harfbuzz_wasm::{Buffer, CGlyphExtents, Font, Glyph, GlyphBuffer};
use kurbo::{
    Affine, BezPath,
    PathSeg::{Cubic, Line, Quad},
};
use wasm_bindgen::prelude::*;

fn get_scaled_outline(font: &Font, glyph: u32, scale_factor: f64) -> Vec<BezPath> {
    let mut paths = font.get_outline(glyph);
    let affine = Affine::scale(scale_factor);
    for path in paths.iter_mut() {
        path.apply_affine(affine);
    }
    paths
}

fn p(p: kurbo::Point) -> ab_glyph_rasterizer::Point {
    ab_glyph_rasterizer::point(p.x as f32, p.y as f32)
}

fn rasterize(
    glyph_metrics: CGlyphExtents,
    paths: &[BezPath],
    cluster: u32,
    pixel_size: f32,
) -> Vec<Glyph> {
    let width = (((glyph_metrics.x_bearing + glyph_metrics.width) as f32 + 0.5) / pixel_size).ceil()
        as usize;
    let height = (-glyph_metrics.height as f32 / pixel_size).ceil() as usize;

    let mut rasterizer = Rasterizer::new(width, height);
    let mut glyphs = vec![];
    // debug(&format!(
    //     "rasterizing:{:?}\nat {:},{:}",
    //     paths, width, height
    // ));
    for path in paths {
        for seg in path.segments() {
            match seg {
                Line(l) => rasterizer.draw_line(p(l.p0), p(l.p1)),
                Quad(q) => rasterizer.draw_quad(p(q.p0), p(q.p1), p(q.p2)),
                Cubic(c) => rasterizer.draw_cubic(p(c.p0), p(c.p1), p(c.p2), p(c.p3)),
            }
        }
    }
    rasterizer.for_each_pixel_2d(|x, y, alpha| {
        if alpha < 0.1 {
            return;
        }
        let color = if alpha > 0.5 {
            2
        } else if alpha > 0.3 {
            3
        } else {
            4
        };
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

    let inner_font_blob = face.reference_table("Font");
    let inner_face = inner_font_blob.into_face(0);
    let inner_font = inner_face.create_font();

    // let (x_scale, _y_scale) = font.get_scale();
    let pixel_size = -font.get_glyph_extents(2).height as f32;
    // debug(&format!("Test pixel size: {}", pixel_size));
    // debug(&format!("Test X scale: {}", x_scale));
    let mut new_glyphs: Vec<Glyph> = vec![];

    // Ordinary shaping
    inner_font.shape_with(buf_ref, "ot");
    let mut buffer: GlyphBuffer = Buffer::from_ref(buf_ref);

    for mut item in buffer.glyphs.iter_mut() {
        let glyph_metrics = inner_font.get_glyph_extents(item.codepoint);
        // debug(&format!("Glyph metrics: {:?}", glyph_metrics));
        // debug(&format!("Raster: {:?}", rasterized));
        // debug(&format!("Raster length: {:?}", rasterized.len()));
        // debug(&format!("Shaped advance: {:?}", item.x_advance));
        // Inner advance width
        let paths = get_scaled_outline(&inner_font, item.codepoint, (1.0 / pixel_size).into());
        item.x_advance = (item.x_advance * face.get_upem() as i32) / (inner_face.get_upem() as i32);
        new_glyphs.extend(rasterize(glyph_metrics, &paths, item.cluster, pixel_size));

        new_glyphs.push(Glyph {
            cluster: item.cluster,
            codepoint: 1, // space,
            x_advance: item.x_advance,
            y_advance: 0,
            flags: 0,
            x_offset: 0,
            y_offset: 0,
        });
    }
    buffer.glyphs = new_glyphs;
    1
}
