use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use bitflags::bitflags;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::grid::{self, GridCell};
use crate::index::Column;
use crate::inline_image::ImageCell;
use crate::vte::ansi::{Color, Hyperlink as VteHyperlink, NamedColor};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
    pub struct Flags: u16 {
        const INVERSE                   = 0b0000_0000_0000_0001;
        const BOLD                      = 0b0000_0000_0000_0010;
        const ITALIC                    = 0b0000_0000_0000_0100;
        const BOLD_ITALIC               = 0b0000_0000_0000_0110;
        const UNDERLINE                 = 0b0000_0000_0000_1000;
        const WRAPLINE                  = 0b0000_0000_0001_0000;
        const WIDE_CHAR                 = 0b0000_0000_0010_0000;
        const WIDE_CHAR_SPACER          = 0b0000_0000_0100_0000;
        const DIM                       = 0b0000_0000_1000_0000;
        const DIM_BOLD                  = 0b0000_0000_1000_0010;
        const HIDDEN                    = 0b0000_0001_0000_0000;
        const STRIKEOUT                 = 0b0000_0010_0000_0000;
        const LEADING_WIDE_CHAR_SPACER  = 0b0000_0100_0000_0000;
        const DOUBLE_UNDERLINE          = 0b0000_1000_0000_0000;
        const UNDERCURL                 = 0b0001_0000_0000_0000;
        const DOTTED_UNDERLINE          = 0b0010_0000_0000_0000;
        const DASHED_UNDERLINE          = 0b0100_0000_0000_0000;
        const ALL_UNDERLINES            = Self::UNDERLINE.bits() | Self::DOUBLE_UNDERLINE.bits()
                                        | Self::UNDERCURL.bits() | Self::DOTTED_UNDERLINE.bits()
                                        | Self::DASHED_UNDERLINE.bits();
    }
}

/// Counter for hyperlinks without explicit ID.
static HYPERLINK_ID_SUFFIX: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Hyperlink {
    inner: Arc<HyperlinkInner>,
}

impl Hyperlink {
    pub fn new<T: ToString>(id: Option<T>, uri: String) -> Self {
        let inner = Arc::new(HyperlinkInner::new(id, uri));
        Self { inner }
    }

    pub fn id(&self) -> &str {
        &self.inner.id
    }

    pub fn uri(&self) -> &str {
        &self.inner.uri
    }
}

impl From<VteHyperlink> for Hyperlink {
    fn from(value: VteHyperlink) -> Self {
        Self::new(value.id, value.uri)
    }
}

impl From<Hyperlink> for VteHyperlink {
    fn from(val: Hyperlink) -> Self {
        VteHyperlink { id: Some(val.id().to_owned()), uri: val.uri().to_owned() }
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct HyperlinkInner {
    /// Identifier for the given hyperlink.
    id: String,

    /// Resource identifier of the hyperlink.
    uri: String,
}

impl HyperlinkInner {
    pub fn new<T: ToString>(id: Option<T>, uri: String) -> Self {
        let id = match id {
            Some(id) => id.to_string(),
            None => {
                let mut id = HYPERLINK_ID_SUFFIX.fetch_add(1, Ordering::Relaxed).to_string();
                id.push_str("_nebula");
                id
            },
        };

        Self { id, uri }
    }
}

/// Trait for determining if a reset should be performed.
pub trait ResetDiscriminant<T> {
    /// Value based on which equality for the reset will be determined.
    fn discriminant(&self) -> T;
}

impl<T: Copy> ResetDiscriminant<T> for T {
    fn discriminant(&self) -> T {
        *self
    }
}

impl ResetDiscriminant<Color> for Cell {
    fn discriminant(&self) -> Color {
        self.bg
    }
}

/// Dynamically allocated cell content.
///
/// This storage is reserved for cell attributes which are rarely set. This allows reducing the
/// allocation required ahead of time for every cell, with some additional overhead when the extra
/// storage is actually required.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CellExtra(Extra);

#[derive(Debug, Clone, Eq, PartialEq)]
enum Extra {
    Text(TextExtra),
    // Image tiles can share text attributes without making CellExtra recursive.
    Image { tile: ImageCell, text: Option<Arc<TextExtra>> },
}

#[derive(Default, Debug, Clone, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize), serde(rename = "CellExtra"))]
struct TextExtra {
    zerowidth: Vec<char>,
    underline_color: Option<Color>,
    hyperlink: Option<Hyperlink>,
}

impl TextExtra {
    fn is_empty(&self) -> bool {
        self.zerowidth.is_empty() && self.underline_color.is_none() && self.hyperlink.is_none()
    }
}

impl Default for CellExtra {
    fn default() -> Self {
        Self(Extra::Text(TextExtra::default()))
    }
}

impl CellExtra {
    #[inline]
    fn text(&self) -> &TextExtra {
        static EMPTY: TextExtra =
            TextExtra { zerowidth: Vec::new(), underline_color: None, hyperlink: None };
        match &self.0 {
            Extra::Text(text) => text,
            Extra::Image { text: Some(text), .. } => text,
            Extra::Image { text: None, .. } => &EMPTY,
        }
    }

    #[inline]
    fn text_mut(&mut self) -> &mut TextExtra {
        match &mut self.0 {
            Extra::Text(text) => text,
            Extra::Image { text, .. } => Arc::make_mut(text.get_or_insert_with(Default::default)),
        }
    }

    fn prune_empty_text(&mut self) {
        if let Extra::Image { text, .. } = &mut self.0 {
            if text.as_ref().is_some_and(|text| text.is_empty()) {
                *text = None;
            }
        }
    }
}

// Keep the existing struct-shaped snapshot format; transient image metadata
// remains omitted, including for tiles which also contain text attributes.
#[cfg(feature = "serde")]
impl Serialize for CellExtra {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.text().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for CellExtra {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        TextExtra::deserialize(deserializer).map(|text| Self(Extra::Text(text)))
    }
}

/// Content and attributes of a single cell in the terminal grid.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
    pub extra: Option<Arc<CellExtra>>,
}

impl Default for Cell {
    #[inline]
    fn default() -> Cell {
        Cell {
            c: ' ',
            bg: Color::Named(NamedColor::Background),
            fg: Color::Named(NamedColor::Foreground),
            flags: Flags::empty(),
            extra: None,
        }
    }
}

impl Cell {
    /// Zerowidth characters stored in this cell.
    #[inline]
    pub fn zerowidth(&self) -> Option<&[char]> {
        self.extra.as_ref().map(|extra| extra.text().zerowidth.as_slice())
    }

    /// Write a new zerowidth character to this cell.
    #[inline]
    pub fn push_zerowidth(&mut self, character: char) {
        let extra = self.extra.get_or_insert(Default::default());
        Arc::make_mut(extra).text_mut().zerowidth.push(character);
    }

    /// Remove all wide char data from a cell.
    #[inline(never)]
    pub fn clear_wide(&mut self) {
        self.flags.remove(Flags::WIDE_CHAR);
        self.discard();
        if let Some(extra) = self.extra.as_mut() {
            Arc::make_mut(extra).text_mut().zerowidth = Vec::new();
        }
        self.c = ' ';
    }

    /// Set underline color on the cell.
    pub fn set_underline_color(&mut self, color: Option<Color>) {
        // If we reset color and we don't have zerowidth we should drop extra storage.
        if color.is_none()
            && self.image().is_none()
            && self.extra.as_ref().is_none_or(|extra| {
                extra.text().zerowidth.is_empty() && extra.text().hyperlink.is_none()
            })
        {
            self.extra = None;
        } else {
            let extra = self.extra.get_or_insert(Default::default());
            let extra = Arc::make_mut(extra);
            extra.text_mut().underline_color = color;
            extra.prune_empty_text();
        }
    }

    /// Underline color stored in this cell.
    #[inline]
    pub fn underline_color(&self) -> Option<Color> {
        self.extra.as_ref()?.text().underline_color
    }

    /// Set hyperlink.
    pub fn set_hyperlink(&mut self, hyperlink: Option<Hyperlink>) {
        let should_drop = hyperlink.is_none()
            && self.image().is_none()
            && self.extra.as_ref().is_none_or(|extra| {
                extra.text().zerowidth.is_empty() && extra.text().underline_color.is_none()
            });

        if should_drop {
            self.extra = None;
        } else {
            let extra = self.extra.get_or_insert(Default::default());
            let extra = Arc::make_mut(extra);
            extra.text_mut().hyperlink = hyperlink;
            extra.prune_empty_text();
        }
    }

    /// Hyperlink stored in this cell.
    #[inline]
    pub fn hyperlink(&self) -> Option<Hyperlink> {
        self.extra.as_ref()?.text().hyperlink.clone()
    }

    #[inline]
    pub fn image(&self) -> Option<&ImageCell> {
        match &self.extra.as_ref()?.0 {
            Extra::Image { tile, .. } => Some(tile),
            Extra::Text(_) => None,
        }
    }

    pub fn set_image(&mut self, image: ImageCell) {
        if self.image().is_some() {
            if let Extra::Image { tile, .. } = &mut Arc::make_mut(self.extra.as_mut().unwrap()).0 {
                *tile = image;
            }
        } else {
            let text = self.extra.take().and_then(|extra| match Arc::unwrap_or_clone(extra).0 {
                Extra::Text(text) => (!text.is_empty()).then(|| Arc::new(text)),
                Extra::Image { .. } => unreachable!("existing image was handled above"),
            });
            self.extra = Some(Arc::new(CellExtra(Extra::Image { tile: image, text })));
        }
    }
}

impl GridCell for Cell {
    #[inline]
    fn discard(&mut self) {
        if let Some(CellExtra(Extra::Image { text, .. })) = self.extra.as_deref() {
            if text.is_none() {
                // Image-only cached rows retain no empty allocation, including
                // when another cell shares this owner: no COW clone is needed.
                self.extra = None;
            } else {
                let extra = Arc::make_mut(self.extra.as_mut().unwrap());
                if let Extra::Image { text, .. } = &mut extra.0 {
                    extra.0 = Extra::Text(Arc::unwrap_or_clone(text.take().unwrap()));
                }
            }
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        (self.c == ' ' || self.c == '\t')
            && self.bg == Color::Named(NamedColor::Background)
            && self.fg == Color::Named(NamedColor::Foreground)
            && !self.flags.intersects(
                Flags::INVERSE
                    | Flags::ALL_UNDERLINES
                    | Flags::STRIKEOUT
                    | Flags::WRAPLINE
                    | Flags::WIDE_CHAR_SPACER
                    | Flags::LEADING_WIDE_CHAR_SPACER,
            )
            && self.extra.as_ref().map(|extra| extra.text().zerowidth.is_empty()) != Some(false)
            && self.image().is_none()
    }

    #[inline]
    fn flags(&self) -> &Flags {
        &self.flags
    }

    #[inline]
    fn flags_mut(&mut self) -> &mut Flags {
        &mut self.flags
    }

    #[inline]
    fn reset(&mut self, template: &Self) {
        *self = Cell { bg: template.bg, ..Cell::default() };
    }
}

impl From<Color> for Cell {
    #[inline]
    fn from(color: Color) -> Self {
        Self { bg: color, ..Cell::default() }
    }
}

/// Get the length of occupied cells in a line.
pub trait LineLength {
    /// Calculate the occupied line length.
    fn line_length(&self) -> Column;
}

impl LineLength for grid::Row<Cell> {
    fn line_length(&self) -> Column {
        let mut length = Column(0);

        if self[Column(self.len() - 1)].flags.contains(Flags::WRAPLINE) {
            return Column(self.len());
        }

        for (index, cell) in self[..].iter().rev().enumerate() {
            if cell.c != ' '
                || cell.extra.as_ref().map(|extra| extra.text().zerowidth.is_empty()) == Some(false)
            {
                length = Column(self.len() - index);
                break;
            }
        }

        length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::mem;

    use crate::grid::Row;
    use crate::index::Column;

    #[test]
    fn cell_size_is_below_cap() {
        // Expected cell size on 64-bit architectures.
        const EXPECTED_CELL_SIZE: usize = 24;

        // Ensure that cell size isn't growing by accident.
        assert!(mem::size_of::<Cell>() <= EXPECTED_CELL_SIZE);
    }

    #[test]
    fn line_length_works() {
        let mut row = Row::<Cell>::new(10);
        row[Column(5)].c = 'a';

        assert_eq!(row.line_length(), Column(6));
    }

    #[test]
    fn line_length_works_with_wrapline() {
        let mut row = Row::<Cell>::new(10);
        row[Column(9)].flags.insert(super::Flags::WRAPLINE);

        assert_eq!(row.line_length(), Column(10));
    }
}
