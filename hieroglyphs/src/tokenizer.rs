#[derive(Debug, Clone)]
pub enum Tok {
    Sign(u32),
    Vert,
    Hor,
    St,
    Sb,
    Et,
    Eb,
    Overlay,
    Begin,
    End,
    Other(u32),
}

pub struct Lexer<'input> {
    chars: Box<dyn Iterator<Item = (usize, &'input u32)> + 'input>,
}

#[derive(Debug)]
pub enum LexicalError {
    // Not possible
}

pub type Spanned<Tok, Loc, Error> = Result<(Loc, Tok, Loc), Error>;

impl<'input> Lexer<'input> {
    pub fn new(input: &'input [u32]) -> Self {
        Lexer {
            chars: Box::new(input.iter().enumerate()),
        }
    }
}

impl<'input> Iterator for Lexer<'input> {
    type Item = Spanned<Tok, usize, LexicalError>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.chars.next() {
            Some((i, 0x13430)) => Some(Ok((i, Tok::Vert, i + 1))),
            Some((i, 0x13431)) => Some(Ok((i, Tok::Hor, i + 1))),
            Some((i, 0x13432)) => Some(Ok((i, Tok::St, i + 1))),
            Some((i, 0x13433)) => Some(Ok((i, Tok::Sb, i + 1))),
            Some((i, 0x13434)) => Some(Ok((i, Tok::Et, i + 1))),
            Some((i, 0x13435)) => Some(Ok((i, Tok::Eb, i + 1))),
            Some((i, 0x13436)) => Some(Ok((i, Tok::Overlay, i + 1))),
            Some((i, 0x13437)) => Some(Ok((i, Tok::Begin, i + 1))),
            Some((i, 0x13438)) => Some(Ok((i, Tok::End, i + 1))),
            Some((i, &cp)) => {
                if (0x13000..=0x1342F).contains(&cp) {
                    Some(Ok((i, Tok::Sign(cp), i + 1)))
                } else {
                    Some(Ok((i, Tok::Other(cp), i + 1)))
                }
            }
            None => None, // End of file
        }
    }
}
