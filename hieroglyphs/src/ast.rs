use crate::tokenizer::Tok;

#[derive(Debug)]
pub enum Expr {
    Sign(Tok),
    Other(Tok),
    Horizontal(Vec<Box<Expr>>),
    Vertical(Vec<Box<Expr>>),
    Overlay(Box<Expr>, Box<Expr>),
    Insertion {
        base: Box<Expr>,
        start_top: Option<Box<Expr>>,
        start_bottom: Option<Box<Expr>>,
        end_top: Option<Box<Expr>>,
        end_bottom: Option<Box<Expr>>,
    },
}

pub fn make_horizontal_group(l: Box<Expr>, r: Vec<Box<Expr>>) -> Box<Expr> {
    let mut l = vec![l];
    l.extend(r);
    Box::new(Expr::Horizontal(l))
}

pub fn make_vertical_group(l: Box<Expr>, r: Vec<Box<Expr>>) -> Box<Expr> {
    let mut l = vec![l];
    l.extend(r);
    Box::new(Expr::Vertical(l))
}
