use crate::ast::Expr;
use harfbuzz_wasm::{debug, Buffer, Font, Glyph, GlyphBuffer};
use std::collections::BTreeMap;

use lalrpop_util::lalrpop_mod;
use wasm_bindgen::prelude::*;
mod tokenizer;
use tokenizer::{Lexer, Tok};
lalrpop_mod!(pub parser);
pub mod ast;

const ADVANCE: f32 = 1500_f32;

struct LayoutEngine<'a> {
    font: &'a Font,
    scale_factor: f32,
    glyphs: Vec<Glyph>,
    half_map: BTreeMap<u32, u32>,
    quarter_map: BTreeMap<u32, u32>,
    width: i32,
    height: i32,
    x_offset: i32,
    y_offset: i32,
    cluster: u32,
    depth: u32,
    is_first_glyph: bool,
}

impl<'a> LayoutEngine<'a> {
    fn new(font: &'a Font) -> Self {
        let names_map: BTreeMap<String, u32> = (0..3210_u32)
            .map(|id| (font.get_glyph_name(id), id))
            .collect();
        let mut half_map: BTreeMap<u32, u32> = BTreeMap::new();
        let mut quarter_map: BTreeMap<u32, u32> = BTreeMap::new();
        let (x_scale, _y_scale) = font.get_scale();
        let face = font.get_face();
        let upem = face.get_upem();
        let scale_factor: f32 = x_scale as f32 / upem as f32;

        // Find IDs for small versions of glyphs
        for (name, &id) in names_map.iter() {
            if let Some(half_id) = names_map.get(&(name.to_owned() + ".half")) {
                half_map.insert(id, *half_id);
            }
            if let Some(quarter_id) = names_map.get(&(name.to_owned() + ".quarter")) {
                quarter_map.insert(id, *quarter_id);
            }
        }

        Self {
            font,
            glyphs: vec![],
            half_map,
            quarter_map,
            scale_factor,
            width: (ADVANCE * scale_factor) as i32,
            height: (ADVANCE * scale_factor) as i32,
            x_offset: 0,
            y_offset: 0,
            cluster: 0,
            depth: 0,
            is_first_glyph: true,
        }
    }
    fn layout_cluster(&mut self, expr: &Expr) {
        self.width = (ADVANCE * self.scale_factor) as i32;
        self.height = (ADVANCE * self.scale_factor) as i32;
        self.x_offset = 0;
        self.y_offset = 0;
        self.depth = 0;
        self.layout_expr(expr);
        self.cluster += 1;
        self.is_first_glyph = true;
    }

    fn layout_expr(&mut self, expr: &Expr) {
        self.depth += 1;
        match expr {
            Expr::Sign(Tok::Sign(s)) => self.layout_glyph(*s),
            Expr::Other(Tok::Other(s)) => self.layout_glyph(*s),
            Expr::Horizontal(lst) => self.layout_horizontal(lst),
            Expr::Vertical(lst) => self.layout_vertical(lst),
            Expr::Overlay(first, second) => self.layout_overlay(first, second),
            Expr::Insertion {
                base,
                start_top,
                start_bottom,
                end_top,
                end_bottom,
            } => self.layout_insertion(base, start_top, start_bottom, end_top, end_bottom),
            _ => {} // Can't happen
        };
        self.depth -= 1;
    }
    fn layout_glyph(&mut self, s: u32) {
        let mut glyph_id = self.font.get_glyph(s, 0);
        // debug(&format!("Laying out CP={:}, depth is={}", s, self.depth));

        // Change size of glyph according to depth. This needs
        // improving.

        // if self.depth > 2 {
        //     glyph_id = *self.quarter_map.get(&glyph_id).unwrap_or(&glyph_id);
        // } else
        //
        if self.depth > 1 {
            glyph_id = *self.half_map.get(&glyph_id).unwrap_or(&glyph_id);
        }

        // let h_advance = self.font.get_glyph_h_advance(glyph_id);
        let h_advance = self.font.get_glyph_extents(glyph_id).width;
        let v_advance = -self.font.get_glyph_extents(glyph_id).height;
        // centering
        let centering_x = (self.width - h_advance) / 2;
        let centering_y = (self.height - v_advance) / 2;
        // debug(&format!(
        //     "Glyph height of {} is {}, cell height is {}, centering by {}",
        //     glyph_id, v_advance, self.height, centering_y
        // ));
        self.glyphs.push(Glyph {
            codepoint: glyph_id,
            cluster: self.cluster,
            x_advance: if self.is_first_glyph {
                (ADVANCE * self.scale_factor) as i32
            } else {
                0
            },
            y_advance: 0,
            x_offset: if !self.is_first_glyph {
                -(ADVANCE * self.scale_factor) as i32
            } else {
                0
            } + self.x_offset
                + centering_x,
            y_offset: self.y_offset + centering_y,
            flags: 0,
        });
        self.is_first_glyph = false
    }

    fn layout_vertical(&mut self, s: &[Box<Expr>]) {
        let count = s.len();
        let step = ((self.height as f32) / (count as f32)) as i32;
        // debug(&format!(
        //     "{:} glyph in stack, base height is {:}, step is {:}",
        //     count, self.height, step
        // ));
        let oldheight = self.height;
        let old_offset = self.y_offset;
        // Top to bottom
        for item in s.iter().rev() {
            self.height = step;
            self.layout_expr(item);
            self.y_offset += step;
        }
        self.height = oldheight;
        self.y_offset = old_offset;
    }

    fn layout_horizontal(&mut self, s: &[Box<Expr>]) {
        let count = s.len();
        let step = ((self.width as f32) / (count as f32)) as i32;
        // debug(&format!(
        //     "{:} glyph in stack, base width is {:}, step is {:}",
        //     count, self.width, step
        // ));
        let oldwidth = self.width;
        let old_offset = self.x_offset;
        for item in s.iter() {
            self.width = step;
            self.depth -= 1; // HACK
            self.layout_expr(item);
            self.depth += 1; // HACK
            self.x_offset += step;
        }
        self.width = oldwidth;
        self.x_offset = old_offset;
    }

    fn layout_overlay(&mut self, first: &Expr, second: &Expr) {
        self.depth -= 1;
        self.layout_expr(first);
        self.layout_expr(second);
        self.depth += 1;
    }

    fn layout_insertion(
        &mut self,
        base: &Expr,
        start_top: &Option<Box<Expr>>,
        start_bottom: &Option<Box<Expr>>,
        end_top: &Option<Box<Expr>>,
        end_bottom: &Option<Box<Expr>>,
    ) {
        self.depth -= 1;
        self.layout_expr(base);
        self.depth += 1;
        self.depth += 1;
        // This is all a terrible hack and could probably
        // be improved with a save stack.
        if let Some(st) = start_top {
            let old_width = self.width;
            let old_height = self.height;
            self.height /= 2;
            self.width /= 2;
            let old_offset = self.y_offset;
            self.y_offset += old_height / 2;
            self.layout_expr(st);
            self.width = old_width;
            self.height = old_height;
            self.y_offset = old_offset;
        }
        if let Some(sb) = start_bottom {
            let old_width = self.width;
            let old_height = self.height;
            self.height /= 2;
            self.width /= 2;
            self.layout_expr(sb);
            self.width = old_width;
            self.height = old_height;
        }

        if let Some(eb) = end_bottom {
            let old_width = self.width;
            let old_offset = self.x_offset;
            self.x_offset += self.width / 2;
            let old_height = self.height;
            self.height /= 2;
            self.width /= 2;
            self.layout_expr(eb);
            self.width = old_width;
            self.height = old_height;
            self.x_offset = old_offset;
        }

        if let Some(et) = end_top {
            let old_width = self.width;
            let old_x_offset = self.x_offset;
            let old_y_offset = self.y_offset;
            self.x_offset += self.width / 2;
            self.y_offset += self.height / 2;
            let old_height = self.height;
            self.height /= 2;
            self.width /= 2;
            self.layout_expr(et);
            self.width = old_width;
            self.height = old_height;
            self.x_offset = old_x_offset;
            self.y_offset = old_y_offset;
        }
        self.depth -= 1;
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
    let font = Font::from_ref(font_ref);
    // Get all glyph names
    let mut buffer = GlyphBuffer::from_ref(buf_ref);
    // Turn the buffer into a nested structure
    let codepoints: Vec<u32> = buffer.glyphs.iter().map(|item| item.codepoint).collect();
    let lexer = Lexer::new(&codepoints);
    let parser = parser::FragmentParser::new();
    let expr: Vec<Box<Expr>> = parser.parse(lexer).unwrap();
    let mut engine = LayoutEngine::new(&font);
    debug(&format!("Expression was {:?}", expr));
    for exp in expr.iter() {
        engine.layout_cluster(exp);
    }
    buffer.glyphs = engine.glyphs;
    1
}
