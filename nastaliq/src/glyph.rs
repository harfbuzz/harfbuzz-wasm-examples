use crate::outline::get_scaled_outline;
use harfbuzz_wasm::{Buffer, BufferItem, CGlyphInfo, CGlyphPosition, Font};
use itertools::Itertools;
use kurbo::{Affine, BezPath, PathEl, PathSeg, Rect};
use std::cell::OnceCell;

// The kinds of glyph we treat differently. A glyph can be of several kinds.
pub const DOT_BELOW: u8 = 1 << 0;
pub const DOT_ABOVE: u8 = 1 << 1;
pub const BARI_YE: u8 = 1 << 2;
pub const INIT: u8 = 1 << 3;
pub const ISOL: u8 = 1 << 4;
pub const FINA: u8 = 1 << 5;
pub const SPACE: u8 = 1 << 6;

// What we need to know about a glyph ID: what kinds of glyph its name
// makes it, and its outline. Finding these out is expensive, and a
// buffer has the same few glyph IDs over and over, so we do it once for
// each glyph ID and let all the glyphs with that ID share the result.
#[derive(Debug)]
pub struct GlyphShape {
    pub kinds: u8,
    // Not every glyph gets asked for its outline, so we only get it
    // from the host the first time somebody does.
    glyph: u32,
    outline: OnceCell<Vec<BezPath>>,
}

impl GlyphShape {
    // These rules will obviously need to be customized
    // for each font, but maybe in a production Nastaliq shaper
    // they would be driven from data in a font table.
    pub fn new(glyph: u32, name: &str) -> Self {
        let mut kinds = 0;
        if name.ends_with("BelowNS") || name.ends_with("HehCommaNS") {
            kinds |= DOT_BELOW;
        }
        if name.ends_with("AboveNS") || name.ends_with("FathaNS") {
            kinds |= DOT_ABOVE;
        }
        // The rest can be anywhere in the name. We look at each place in
        // the name once, for all of them, rather than search the name
        // five times: this is what name.contains("Ini") and so on say.
        let bytes = name.as_bytes();
        for at in 0..bytes.len() {
            let rest = &bytes[at..];
            match bytes[at] {
                b'Y' if rest.starts_with(b"YehBarreeFin") => kinds |= BARI_YE,
                b'I' if rest.starts_with(b"Ini") => kinds |= INIT,
                b'S' if rest.starts_with(b"Sep") => kinds |= ISOL,
                b'F' if rest.starts_with(b"Fin") => kinds |= FINA,
                b's' if rest.starts_with(b"space") => kinds |= SPACE,
                _ => {}
            }
        }
        Self {
            kinds,
            glyph,
            outline: OnceCell::new(),
        }
    }

    // The glyph's outline, scaled up a little.
    pub fn outline(&self, font: &Font) -> &Vec<BezPath> {
        self.outline
            .get_or_init(|| get_scaled_outline(font, self.glyph))
    }
}

// This is the standard glyph representation but with a few more
// handy fields.
#[derive(Debug)]
pub struct GulzarGlyph {
    pub codepoint: u32,
    // Which GlyphShape this glyph has, and a copy of its kinds.
    pub shape: usize,
    pub kinds: u8,
    pub cluster: u32,
    pub x_advance: i32,
    pub x_total_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub in_bari_ye: bool,
}

impl BufferItem for GulzarGlyph {
    fn from_c(info: CGlyphInfo, pos: CGlyphPosition) -> Self {
        Self {
            codepoint: info.codepoint,
            shape: 0,
            kinds: 0,
            cluster: info.cluster,
            x_advance: pos.x_advance,
            x_total_advance: 0,
            y_advance: pos.y_advance,
            x_offset: pos.x_offset,
            y_offset: pos.y_offset,
            in_bari_ye: false,
        }
    }
    fn to_c(self) -> (CGlyphInfo, CGlyphPosition) {
        let info = CGlyphInfo {
            codepoint: self.codepoint,
            cluster: self.cluster,
            mask: 0,
            var1: 0,
            var2: 0,
        };
        let pos = CGlyphPosition {
            x_advance: self.x_advance,
            y_advance: self.y_advance,
            x_offset: self.x_offset,
            y_offset: self.y_offset,
            var: 0,
        };
        (info, pos)
    }
}

impl GulzarGlyph {
    pub fn is_dot_below(&self) -> bool {
        self.kinds & DOT_BELOW != 0
    }

    pub fn is_dot_above(&self) -> bool {
        self.kinds & DOT_ABOVE != 0
    }

    pub fn is_bari_ye(&self) -> bool {
        self.kinds & BARI_YE != 0
    }

    pub fn is_init(&self) -> bool {
        self.kinds & INIT != 0
    }

    pub fn is_isol(&self) -> bool {
        self.kinds & ISOL != 0
    }

    pub fn is_fina(&self) -> bool {
        self.kinds & FINA != 0
    }

    pub fn is_space(&self) -> bool {
        self.kinds & SPACE != 0
    }

    // This is the bounding box of the *positioned* glyph,
    // i.e. including the full running-total x advance.
    pub fn bounding_box(&self, font: &Font) -> Rect {
        let extents = font.get_glyph_extents(self.codepoint);
        let bl_x = extents.x_bearing + self.x_total_advance + self.x_offset;
        let bl_y = extents.y_bearing + extents.height + self.y_offset;
        let tr_x = bl_x + extents.width;
        let tr_y = bl_y - extents.height;
        Rect::from_points((bl_x as f64, bl_y as f64), (tr_x as f64, tr_y as f64))
    }

    // Similarly, kurbo representations of a glyph's paths
    // positioned absolutely.
    pub fn positioned_paths(&self, shapes: &[GlyphShape], font: &Font) -> Vec<BezPath> {
        let mut paths = shapes[self.shape].outline(font).clone(); // urgh
        let affine = Affine::translate((
            (self.x_total_advance + self.x_offset) as f64,
            self.y_offset as f64,
        ));
        for p in paths.iter_mut() {
            p.apply_affine(affine);
        }
        paths
    }

    // Terribly inefficient collision detector
    pub fn collides(&self, other: &GulzarGlyph, shapes: &[GlyphShape], font: &Font) -> bool {
        // If the bounding boxes don't intersect, we can't collide.
        if self
            .bounding_box(font)
            .intersect(other.bounding_box(font))
            .area()
            == 0.0
        {
            return false;
        }
        let (sx, _sy) = font.get_scale();

        let my_paths = self.positioned_paths(shapes, font);
        let their_paths = other.positioned_paths(shapes, font);
        // We could do line sweep or something here, but proof of concept...
        for p1 in my_paths {
            for p2 in &their_paths {
                if intersects(&p1, p2, 50.0 * (sx as f64)) {
                    return true;
                }
            }
        }
        false
    }
}
pub type GulzarBuffer = Buffer<GulzarGlyph>;

fn intersects(b1: &BezPath, b2: &BezPath, scale: f64) -> bool {
    let mut pts1 = vec![];
    let mut pts2 = vec![];
    b1.flatten(scale, |el| match el {
        PathEl::MoveTo(a) => pts1.push(a),
        PathEl::LineTo(a) => pts1.push(a),
        _ => {}
    });
    b2.flatten(scale, |el| match el {
        PathEl::MoveTo(a) => pts2.push(a),
        PathEl::LineTo(a) => pts2.push(a),
        _ => {}
    });
    for (&la1, &la2) in pts1.iter().circular_tuple_windows() {
        for (&lb1, &lb2) in pts2.iter().circular_tuple_windows() {
            let seg1 = PathSeg::Line(kurbo::Line::new(la1, la2));
            let seg2 = kurbo::Line::new(lb1, lb2);
            if !seg1.intersect_line(seg2).is_empty() {
                return true;
            }
        }
    }
    false
}
