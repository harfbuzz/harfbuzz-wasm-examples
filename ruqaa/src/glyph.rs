use harfbuzz_wasm::{Buffer, BufferItem, CGlyphExtents, CGlyphInfo, CGlyphPosition, Font};
use itertools::Itertools;
use kurbo::{Affine, BezPath, PathEl, PathSeg, Rect};

#[derive(Debug)]
pub struct GulzarGlyph {
    pub codepoint: u32,
    pub name: String,
    pub paths: Vec<BezPath>,
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
            name: "".to_string(),
            paths: vec![],
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
        self.name.ends_with("below-ar")
    }

    pub fn is_dot_above(&self) -> bool {
        self.name.ends_with("above-ar")
    }

    pub fn is_dot(&self) -> bool {
        self.name.ends_with("above-ar") || self.name.ends_with("below-ar")
    }

    pub fn bounding_box(&self, font: &Font) -> Rect {
        let extents = font.get_glyph_extents(self.codepoint);
        let bl_x = extents.x_bearing + self.x_total_advance + self.x_offset;
        let bl_y = extents.y_bearing + extents.height + self.y_offset;
        let tr_x = bl_x + extents.width;
        let tr_y = bl_y - extents.height;
        Rect::from_points((bl_x as f64, bl_y as f64), (tr_x as f64, tr_y as f64))
    }

    pub fn positioned_paths(&self) -> Vec<BezPath> {
        let mut paths = self.paths.clone(); // urgh
        let affine = Affine::translate((
            (self.x_total_advance + self.x_offset) as f64,
            self.y_offset as f64,
        ));
        for p in paths.iter_mut() {
            p.apply_affine(affine);
        }
        paths
    }

    pub fn collides(&self, other: &GulzarGlyph, font: &Font) -> bool {
        if self
            .bounding_box(font)
            .intersect(other.bounding_box(font))
            .area()
            == 0.0
        {
            return false;
        }
        let (sx, sy) = font.get_scale();

        let my_paths = self.positioned_paths();
        let their_paths = other.positioned_paths();
        // We could do line sweep or something here, but proof of concept...
        for p1 in my_paths {
            for p2 in &their_paths {
                if intersects(&p1, p2, 10.0 * (sx as f64)) {
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
