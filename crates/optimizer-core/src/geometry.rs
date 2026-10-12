//! Exact, deterministic 2D cutting primitives (Stage 4).
//!
//! All dimensions/coordinates are integer 0.1 mm units. A cut position is
//! the distance from a region's left/top boundary to the *near edge* of the
//! saw's removed channel, not its center. Cuts need two nonempty children.
//! This module does NOT validate guillotine-stage order or machine execution.

use crate::model::{CutDirection, TrimMargins, Unit};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    EmptyRectangle,
    CoordinateOverflow,
    InvalidMargins,
    InvalidKerf,
    InvalidSplit,
    OutsideParent,
}

impl fmt::Display for GeometryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyRectangle => "rectangle width and height must be positive",
            Self::CoordinateOverflow => "rectangle coordinates exceed supported u32 range",
            Self::InvalidMargins => "insets must leave positive width and height",
            Self::InvalidKerf => "saw kerf must be positive",
            Self::InvalidSplit => "cut and kerf must leave two positive-size regions",
            Self::OutsideParent => "rectangle must lie entirely inside the parent",
        };
        write!(f, "{message}")
    }
}
impl std::error::Error for GeometryError {}

/// Physical coordinate frame: +X right, +Y down. 1 unit = 0.1 mm.
/// Constructors/operations ensure x+width, y+height do not overflow u32.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: Unit,
    pub y: Unit,
    pub width: Unit,
    pub height: Unit,
}

impl Rect {
    pub fn new(x: Unit, y: Unit, width: Unit, height: Unit) -> Result<Self, GeometryError> {
        let rect = Self { x, y, width, height };
        rect.validate()?;
        Ok(rect)
    }

    /// Important: Serde deserialization bypasses `new`. Call this at all
    /// public entry points receiving an externally supplied Rect.
    pub fn validate(&self) -> Result<(), GeometryError> {
        if self.width == 0 || self.height == 0 {
            return Err(GeometryError::EmptyRectangle);
        }
        self.x.checked_add(self.width).ok_or(GeometryError::CoordinateOverflow)?;
        self.y.checked_add(self.height).ok_or(GeometryError::CoordinateOverflow)?;
        Ok(())
    }

    pub fn right(&self) -> Result<Unit, GeometryError> {
        self.validate()?;
        Ok(self.x + self.width)
    }

    pub fn bottom(&self) -> Result<Unit, GeometryError> {
        self.validate()?;
        Ok(self.y + self.height)
    }

    /// u128 avoids overflow for an area of two u32 dimensions.
    pub fn area_sq_units(&self) -> u128 {
        u128::from(self.width) * u128::from(self.height)
    }

    pub fn contains(&self, other: &Self) -> Result<bool, GeometryError> {
        self.validate()?;
        other.validate()?;
        Ok(other.x >= self.x && other.y >= self.y
            && other.right()? <= self.right()?
            && other.bottom()? <= self.bottom()?)
    }

    /// Area intersection only; touching edges are not overlapping.
    pub fn intersection(&self, other: &Self) -> Result<Option<Self>, GeometryError> {
        self.validate()?;
        other.validate()?;
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = self.right()?.min(other.right()?);
        let bottom = self.bottom()?.min(other.bottom()?);
        if left >= right || top >= bottom {
            return Ok(None);
        }
        Ok(Some(Self::new(left, top, right - left, bottom - top)?))
    }

    pub fn intersects(&self, other: &Self) -> Result<bool, GeometryError> {
        Ok(self.intersection(other)?.is_some())
    }

    pub fn translated(&self, dx: i64, dy: i64) -> Result<Self, GeometryError> {
        self.validate()?;
        fn shift(v: Unit, delta: i64) -> Result<Unit, GeometryError> {
            let n = i128::from(v) + i128::from(delta);
            Unit::try_from(n).map_err(|_| GeometryError::CoordinateOverflow)
        }
        Self::new(shift(self.x, dx)?, shift(self.y, dy)?, self.width, self.height)
    }

    /// Trim a rectangle, preserving absolute coordinates. Margins are
    /// physical insets, not saw-kerf operations (saneado is planned later).
    pub fn inset(&self, margin: TrimMargins) -> Result<Self, GeometryError> {
        self.validate()?;
        let horizontal = margin.left.checked_add(margin.right).ok_or(GeometryError::InvalidMargins)?;
        let vertical = margin.top.checked_add(margin.bottom).ok_or(GeometryError::InvalidMargins)?;
        if horizontal >= self.width || vertical >= self.height {
            return Err(GeometryError::InvalidMargins);
        }
        Self::new(
            self.x.checked_add(margin.left).ok_or(GeometryError::CoordinateOverflow)?,
            self.y.checked_add(margin.top).ok_or(GeometryError::CoordinateOverflow)?,
            self.width - horizontal,
            self.height - vertical,
        )
    }

    /// Rotate an enclosed child 90° clockwise along with its parent.
    /// Coordinates remain relative to the same parent origin; the parent's
    /// width and height exchange. A geometric operation only (not a machine move).
    pub fn rotate_cw_within(&self, parent: &Self) -> Result<Self, GeometryError> {
        if !parent.contains(self)? {
            return Err(GeometryError::OutsideParent);
        }
        let rel_y = self.y - parent.y;
        let rel_x = self.x - parent.x;
        let rotated_x = parent.x.checked_add(parent.height - rel_y - self.height)
            .ok_or(GeometryError::CoordinateOverflow)?;
        let rotated_y = parent.y.checked_add(rel_x)
            .ok_or(GeometryError::CoordinateOverflow)?;
        let rotated_parent = Self::new(parent.x, parent.y, parent.height, parent.width)?;
        let rotated = Self::new(rotated_x, rotated_y, self.height, self.width)?;
        if !rotated_parent.contains(&rotated)? {
            return Err(GeometryError::OutsideParent);
        }
        Ok(rotated)
    }

    /// Split into first region, consumed saw channel, and second region.
    /// Vertical: first is LEFT, second RIGHT. Horizontal: TOP then BOTTOM.
    /// `at` is a relative offset to the beginning of the kerf channel.
    pub fn split(&self, direction: CutDirection, at: Unit, kerf: Unit) -> Result<SplitResult, GeometryError> {
        self.validate()?;
        if kerf == 0 {
            return Err(GeometryError::InvalidKerf);
        }
        let extent = match direction { CutDirection::Vertical => self.width, CutDirection::Horizontal => self.height };
        let consumed = at.checked_add(kerf).ok_or(GeometryError::InvalidSplit)?;
        if at == 0 || consumed >= extent {
            return Err(GeometryError::InvalidSplit);
        }
        let remainder = extent - consumed;
        let (first, saw_path, second) = match direction {
            CutDirection::Vertical => (
                Self::new(self.x, self.y, at, self.height)?,
                Self::new(self.x + at, self.y, kerf, self.height)?,
                Self::new(self.x + consumed, self.y, remainder, self.height)?,
            ),
            CutDirection::Horizontal => (
                Self::new(self.x, self.y, self.width, at)?,
                Self::new(self.x, self.y + at, self.width, kerf)?,
                Self::new(self.x, self.y + consumed, self.width, remainder)?,
            ),
        };
        debug_assert_eq!(self.area_sq_units(), first.area_sq_units() + saw_path.area_sq_units() + second.area_sq_units());
        Ok(SplitResult { direction, at, kerf, first, saw_path, second })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SplitResult {
    pub direction: CutDirection,
    pub at: Unit,
    pub kerf: Unit,
    pub first: Rect,
    pub saw_path: Rect,
    pub second: Rect,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn r(x: Unit, y: Unit, w: Unit, h: Unit) -> Rect { Rect::new(x,y,w,h).unwrap() }
    #[test]
    fn vertical_split_absolute_positions_and_area() {
        let source = r(100,200,1000,600);
        let s = source.split(CutDirection::Vertical,400,32).unwrap();
        assert_eq!(s.first, r(100,200,400,600));
        assert_eq!(s.saw_path, r(500,200,32,600));
        assert_eq!(s.second, r(532,200,568,600));
        assert_eq!(source.area_sq_units(),s.first.area_sq_units()+s.saw_path.area_sq_units()+s.second.area_sq_units());
    }
    #[test]
    fn horizontal_split_kerf_is_removed() {
        let s = r(70,80,900,700).split(CutDirection::Horizontal,200,30).unwrap();
        assert_eq!(s.first,r(70,80,900,200));
        assert_eq!(s.saw_path,r(70,280,900,30));
        assert_eq!(s.second,r(70,310,900,470));
    }
    #[test]
    fn cut_at_zero_is_rejected() {
        assert_eq!(r(0,0,100,100).split(CutDirection::Vertical,0,3), Err(GeometryError::InvalidSplit));
    }
    #[test]
    fn zero_kerf_rejected() {
        assert_eq!(r(0,0,100,100).split(CutDirection::Vertical,30,0), Err(GeometryError::InvalidKerf));
    }
    #[test]
    fn cut_at_end_or_with_empty_second_region_is_rejected() {
        for at in [97,98,100] {
            assert_eq!(r(0,0,100,100).split(CutDirection::Vertical,at,3), Err(GeometryError::InvalidSplit));
        }
    }
    #[test]
    fn cut_overflow_is_rejected() {
        assert_eq!(r(0,0,100,100).split(CutDirection::Vertical,Unit::MAX,3), Err(GeometryError::InvalidSplit));
    }
    #[test]
    fn invalid_rectangles_rejected() {
        assert_eq!(Rect::new(0,0,0,2),Err(GeometryError::EmptyRectangle));
        assert_eq!(Rect::new(0,0,3,0),Err(GeometryError::EmptyRectangle));
        assert_eq!(Rect::new(Unit::MAX,0,3,1),Err(GeometryError::CoordinateOverflow));
    }
    #[test]
    fn deserialized_rect_needs_validation() {
        let v: Rect = serde_json::from_str(r#"{"x":4294967295,"y":0,"width":12,"height":10}"#).unwrap();
        assert_eq!(v.validate(), Err(GeometryError::CoordinateOverflow));
    }
    #[test]
    fn inset_asymmetric_and_absolute_origin() {
        let trimmed = r(50,70,1000,500).inset(TrimMargins{left:10, top:20, right:30, bottom:40}).unwrap();
        assert_eq!(trimmed, r(60,90,960,440));
    }
    #[test]
    fn inset_exact_width_rejected() {
        assert_eq!(r(0,0,100,100).inset(TrimMargins{left:80,right:20,..TrimMargins::default()}),Err(GeometryError::InvalidMargins));
    }
    #[test]
    fn inset_overflow_rejected() {
        assert_eq!(r(0,0,100,100).inset(TrimMargins{left:Unit::MAX,right:1,..TrimMargins::default()}),Err(GeometryError::InvalidMargins));
    }
    #[test]
    fn containment_and_edge_touching_intersection() {
        let a = r(1,1,100,50);
        assert!(a.contains(&r(10,10,20,20)).unwrap());
        assert!(!a.contains(&r(90,40,20,20)).unwrap());
        assert!(!a.intersects(&r(101,1,30,50)).unwrap());
        assert_eq!(a.intersection(&r(91,30,20,50)).unwrap(),Some(r(91,30,10,21)));
    }
    #[test]
    fn translation_positive_negative_and_underflow() {
        assert_eq!(r(30,40,50,60).translated(-20,5).unwrap(),r(10,45,50,60));
        assert_eq!(r(30,40,50,60).translated(-31,0),Err(GeometryError::CoordinateOverflow));
        assert_eq!(r(30,40,50,60).translated(i64::MAX,0),Err(GeometryError::CoordinateOverflow));
    }
    #[test]
    fn rotation_of_child_and_parent_agrees() {
        let parent = r(100,200,500,300);
        let child = r(130,240,200,100);
        let rotated = child.rotate_cw_within(&parent).unwrap();
        assert_eq!(rotated,r(260,230,100,200));
        assert_eq!(rotated.area_sq_units(),child.area_sq_units());
    }
    #[test]
    fn rotation_outside_parent_rejected() {
        assert_eq!(r(99,200,10,10).rotate_cw_within(&r(100,200,500,300)),Err(GeometryError::OutsideParent));
    }
    #[test]
    fn area_uses_wide_integer() {
        assert_eq!(r(0,0,Unit::MAX,Unit::MAX).area_sq_units(),u128::from(Unit::MAX).pow(2));
    }
    #[test]
    fn exhaustive_small_rectangles_and_cuts_conserve_area_and_do_not_overlap() {
        for w in 2..24u32 {
            for h in 2..24u32 {
                let original = r(15,27,w,h);
                for dir in [CutDirection::Vertical,CutDirection::Horizontal] {
                    let extent = if dir == CutDirection::Vertical {w} else {h};
                    for kerf in 1..extent {
                        for at in 1..extent {
                            let got = original.split(dir,at,kerf);
                            if at+kerf < extent {
                                let s = got.unwrap();
                                assert_eq!(original.area_sq_units(),s.first.area_sq_units()+s.saw_path.area_sq_units()+s.second.area_sq_units());
                                assert!(original.contains(&s.first).unwrap());
                                assert!(original.contains(&s.second).unwrap());
                                assert!(original.contains(&s.saw_path).unwrap());
                                assert!(!s.first.intersects(&s.second).unwrap());
                                assert!(!s.first.intersects(&s.saw_path).unwrap());
                                assert!(!s.second.intersects(&s.saw_path).unwrap());
                            } else {
                                assert_eq!(got,Err(GeometryError::InvalidSplit));
                            }
                        }
                    }
                }
            }
        }
    }
}
