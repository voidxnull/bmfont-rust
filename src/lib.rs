//! Parser for bitmap fonts

mod char;
mod config_parse_error;
mod error;
mod kerning_value;
mod page;
mod rect;
mod sections;
mod utils;

pub use self::config_parse_error::ConfigParseError;
pub use self::error::Error;
pub use self::rect::Rect;

use std::io::Read;
use self::char::Char;
use self::kerning_value::KerningValue;
use self::page::Page;
use self::sections::Sections;
use std::str::Chars;
use std::str::Lines;

#[derive(Clone, Debug)]
pub struct CharPosition {
    pub page_rect: Rect,
    pub screen_rect: Rect,
    pub page_index: u32,
}

#[derive(Clone, Debug)]
pub enum OrdinateOrientation {
    BottomToTop,
    TopToBottom,
}

#[derive(Clone, Debug)]
pub struct BMFont {
    base_height: u32,
    line_height: u32,
    characters: Vec<Char>,
    kerning_values: Vec<KerningValue>,
    pages: Vec<Page>,
    ordinate_orientation: OrdinateOrientation,
}

impl BMFont {
    pub fn new<R>(source: R, ordinate_orientation: OrdinateOrientation) -> Result<BMFont, Error>
        where R: Read
    {
        let sections = try!(Sections::new(source));

        let base_height;
        let line_height;
        {
            let mut components = sections.common_section.split_whitespace();
            components.next();
            line_height = try!(utils::extract_component_value(components.next(), "common", "lineHeight"));
            base_height = try!(utils::extract_component_value(components.next(), "common", "base"));
        }

        let mut pages = Vec::new();
        for page_section in &sections.page_sections {
            pages.push(try!(Page::new(page_section)));
        }
        let mut characters = Vec::new();
        for char_section in &sections.char_sections {
            characters.push(try!(Char::new(char_section)));
        }
        let mut kerning_values = Vec::new();
        for kerning_section in &sections.kerning_sections {
            kerning_values.push(try!(KerningValue::new(kerning_section)));
        }
        Ok(BMFont {
            base_height: base_height,
            line_height: line_height,
            characters: characters,
            kerning_values: kerning_values,
            pages: pages,
            ordinate_orientation: ordinate_orientation,
        })
    }

    /// Returns the height of a `EM` in pixels.
    pub fn base_height(&self) -> u32 {
        self.base_height
    }

    pub fn line_height(&self) -> u32 {
        self.line_height
    }

    pub fn pages(&self) -> &[Page] {
        self.pages.as_slice()
    }

    pub fn char_positions<'str, 'font>(&'font self, string: &'str str) -> CharPositions<'str, 'font> {
        CharPositions::new(string, self)
    }

    fn find_kerning_values(&self, first_char_id: u32) -> Vec<&KerningValue> {
        self.kerning_values.iter().filter(|k| k.first_char_id == first_char_id).collect()
    }
}

pub struct TextLines<'str, 'font> {
    all_chars: &'font [Char],
    lines: Lines<'str>,
}

impl<'str, 'font> TextLines<'str, 'font> {
    fn new(string: &'str str, all_chars: &'font [Char]) -> Self {
        TextLines {
            all_chars,
            lines: string.lines(),
        }
    }
}

impl<'str, 'font> Iterator for TextLines<'str, 'font> {
    type Item = TextLine<'str, 'font>;

    fn next(&mut self) -> Option<TextLine<'str, 'font>> {
        let substring = self.lines.next()?;
        let line = TextLine::new(substring, self.all_chars);
        Some(line)
    }
}

pub struct TextLine<'str, 'font> {
    all_chars: &'font [Char],
    chars: Chars<'str>,
}

impl<'str, 'font> TextLine<'str, 'font> {
    fn new(string: &'str str, all_chars: &'font [Char]) -> Self {
        TextLine {
            all_chars,
            chars: string.chars(),
        }
    }
}

impl<'str, 'font> Iterator for TextLine<'str, 'font> {
    type Item = Result<&'font Char, CharError>;

    fn next(&mut self) -> Option<Result<&'font Char, CharError>> {
        let c = self.chars.next()?;

        if c.len_utf16() != 1 {
            return Some(Err(CharError::UnsupportedCharacter(c)));
        }

        let char_id = c as u32;
        if let Some(found_char) = self.all_chars.iter().find(|c| c.id == char_id) {
            return Some(Ok(found_char));
        } else {
            return Some(Err(CharError::MissingCharacter(c)));
        }
    }
}

pub struct CharPositions<'str, 'font> {
    font: &'font BMFont,
    text_lines: TextLines<'str, 'font>,
    text_line: TextLine<'str, 'font>,
    x: i32,
    y: i32,
    prev_char_id: u32,
}

impl<'str, 'font> CharPositions<'str, 'font> {
    fn new(string: &'str str, font: &'font BMFont) -> Self {
        let mut text_lines = TextLines::new(string, &font.characters);
        let text_line = text_lines.next().unwrap(); // FIXME

        CharPositions {
            font,
            text_lines,
            text_line,
            x: 0,
            y: 0,
            prev_char_id: 0,
        }
    }
}

impl<'font, 'str> Iterator for CharPositions<'font, 'str> {
    type Item = Result<CharPosition, CharError>;

    fn next(&mut self) -> Option<Result<CharPosition, CharError>> {
        let character = match self.text_line.next() {
            Some(char) => match char {
                Ok(char) => char,
                Err(err) => return Some(Err(err)),
            },
            None => {
                self.text_line = self.text_lines.next()?;
                self.x = 0;

                match self.font.ordinate_orientation {
                    OrdinateOrientation::TopToBottom => self.y += self.font.line_height as i32,
                    OrdinateOrientation::BottomToTop => self.y -= self.font.line_height as i32,
                }

                match self.text_line.next()? {
                    Ok(char) => char,
                    Err(err) => return Some(Err(err)),
                }
            },
        };

        let (x, y) = (self.x, self.y);

        let kerning_value = self.font.kerning_values.iter()
            .find(|k| k.first_char_id == self.prev_char_id && k.second_char_id == character.id)
            .map(|k| k.value)
            .unwrap_or(0);
        let page_rect = Rect {
            x: character.x as i32,
            y: character.y as i32,
            width: character.width,
            height: character.height,
        };
        let screen_x = x + character.xoffset + kerning_value;
        let screen_y = match self.font.ordinate_orientation {
            OrdinateOrientation::BottomToTop => {
                y + self.font.base_height as i32 - character.yoffset - character.height as i32
            }
            OrdinateOrientation::TopToBottom => y + character.yoffset,
        };
        let screen_rect = Rect {
            x: screen_x,
            y: screen_y,
            width: character.width,
            height: character.height,
        };
        let char_position = CharPosition {
            page_rect,
            screen_rect,
            page_index: character.page_index,
        };

        self.x += character.xadvance + kerning_value;
        self.prev_char_id = character.id;

        Some(Ok(char_position))
    }
}

#[derive(Debug)]
pub enum CharError {
    UnsupportedCharacter(char),
    MissingCharacter(char),
}
