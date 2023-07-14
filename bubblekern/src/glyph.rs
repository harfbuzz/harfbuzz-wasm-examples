use harfbuzz_wasm::{Buffer, BufferItem, CGlyphExtents, CGlyphInfo, CGlyphPosition, Font};
use itertools::Itertools;
use kurbo::{Affine, BezPath, PathEl, PathSeg, Rect};

#[derive(Debug)]
pub struct BubbleGlyph {
    pub codepoint: u32,
    pub name: String,
    pub bubble_paths: Option<Vec<BezPath>>,
    pub cluster: u32,
    pub x_advance: i32,
    pub x_total_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

impl BufferItem for BubbleGlyph {
    fn from_c(info: CGlyphInfo, pos: CGlyphPosition) -> Self {
        Self {
            codepoint: info.codepoint,
            name: "".to_string(),
            bubble_paths: None,
            cluster: info.cluster,
            x_advance: pos.x_advance,
            x_total_advance: 0,
            y_advance: pos.y_advance,
            x_offset: pos.x_offset,
            y_offset: pos.y_offset,
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

impl BubbleGlyph {
    pub fn positioned_bubble_paths(&self, starting_kern: f32) -> Option<Vec<BezPath>> {
        self.bubble_paths.as_ref().map(|bp| {
            let mut paths = bp.clone();
            let affine = Affine::translate((
                (self.x_total_advance + self.x_offset) as f64 + starting_kern as f64,
                self.y_offset as f64,
            ));
            for p in paths.iter_mut() {
                p.apply_affine(affine);
            }
            paths
        })
    }
}

pub type BubbleBuffer = Buffer<BubbleGlyph>;
