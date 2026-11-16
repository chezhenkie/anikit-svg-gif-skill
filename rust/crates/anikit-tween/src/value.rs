//! Typed property values and the arithmetic they share.
//!
//! Time and easing live in the track; a value only knows how to interpolate it
//! and how to combine with another value of the same kind.

use crate::track::Property;

/// A premultiplied colour in linear light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    /// Straight linear RGB and alpha, premultiplied on the way in.
    pub fn linear(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color {
            r: r * a,
            g: g * a,
            b: b * a,
            a,
        }
    }

    /// An sRGB code value with alpha, stored premultiplied in linear light.
    pub fn srgb8(r: u8, g: u8, b: u8, a: u8) -> Color {
        let alpha = a as f32 / 255.0;
        Color::linear(
            srgb_to_linear(r as f32 / 255.0),
            srgb_to_linear(g as f32 / 255.0),
            srgb_to_linear(b as f32 / 255.0),
            alpha,
        )
    }

    /// Back to an sRGB code value, unpremultiplied and rounded.
    pub fn to_srgb8(&self) -> [u8; 4] {
        let alpha = self.a.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return [0, 0, 0, 0];
        }
        [
            encode(self.r / alpha),
            encode(self.g / alpha),
            encode(self.b / alpha),
            (alpha * 255.0).round() as u8,
        ]
    }

    pub fn alpha(&self) -> f32 {
        self.a
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let srgb = if value <= 0.0031308 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round().clamp(0.0, 255.0) as u8
}

impl Property for Color {
    fn zero() -> Color {
        Color::TRANSPARENT
    }

    fn add_scaled(&mut self, other: &Color, k: f64) {
        let k = k as f32;
        self.r += other.r * k;
        self.g += other.g * k;
        self.b += other.b * k;
        self.a += other.a * k;
    }
}

/// A 2D affine transform in column-vector form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub m11: f64,
    pub m12: f64,
    pub m21: f64,
    pub m22: f64,
    pub tx: f64,
    pub ty: f64,
}

/// A transform split into the parts that interpolate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decomposed {
    pub tx: f64,
    pub ty: f64,
    pub rotation: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub skew: f64,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        m11: 1.0,
        m12: 0.0,
        m21: 0.0,
        m22: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// The additive zero, used by spline arithmetic. It is not the identity.
    pub const ZERO: Transform = Transform {
        m11: 0.0,
        m12: 0.0,
        m21: 0.0,
        m22: 0.0,
        tx: 0.0,
        ty: 0.0,
    };

    pub fn translate(tx: f64, ty: f64) -> Transform {
        Transform {
            tx,
            ty,
            ..Transform::IDENTITY
        }
    }

    pub fn scale(sx: f64, sy: f64) -> Transform {
        Transform {
            m11: sx,
            m22: sy,
            ..Transform::IDENTITY
        }
    }

    pub fn rotate(radians: f64) -> Transform {
        let (sin, cos) = radians.sin_cos();
        Transform {
            m11: cos,
            m12: sin,
            m21: -sin,
            m22: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    /// Multiply, applying `rhs` first and then `self`.
    pub fn mul(&self, rhs: &Transform) -> Transform {
        Transform {
            m11: self.m11 * rhs.m11 + self.m21 * rhs.m12,
            m12: self.m12 * rhs.m11 + self.m22 * rhs.m12,
            m21: self.m11 * rhs.m21 + self.m21 * rhs.m22,
            m22: self.m12 * rhs.m21 + self.m22 * rhs.m22,
            tx: self.m11 * rhs.tx + self.m21 * rhs.ty + self.tx,
            ty: self.m12 * rhs.tx + self.m22 * rhs.ty + self.ty,
        }
    }

    pub fn apply(&self, x: f64, y: f64) -> [f64; 2] {
        [
            self.m11 * x + self.m21 * y + self.tx,
            self.m12 * x + self.m22 * y + self.ty,
        ]
    }

    /// Split into translation, rotation, scale and skew.
    pub fn decompose(&self) -> Decomposed {
        let scale_x = self.m11.hypot(self.m12);
        if scale_x == 0.0 {
            return Decomposed {
                tx: self.tx,
                ty: self.ty,
                rotation: 0.0,
                scale_x: 0.0,
                scale_y: self.m21.hypot(self.m22),
                skew: 0.0,
            };
        }
        let rotation = self.m12.atan2(self.m11);
        let (sin, cos) = rotation.sin_cos();
        let row1_x = self.m21 * cos + self.m22 * sin;
        let scale_y = -self.m21 * sin + self.m22 * cos;
        let skew = if scale_y.abs() > 0.0 {
            (row1_x / scale_y).atan()
        } else {
            0.0
        };
        Decomposed {
            tx: self.tx,
            ty: self.ty,
            rotation,
            scale_x,
            scale_y,
            skew,
        }
    }

    pub fn from_decomposed(value: &Decomposed) -> Transform {
        let (sin, cos) = value.rotation.sin_cos();
        let skew = value.skew.tan();
        Transform {
            m11: cos * value.scale_x,
            m12: sin * value.scale_x,
            m21: cos * skew * value.scale_y - sin * value.scale_y,
            m22: sin * skew * value.scale_y + cos * value.scale_y,
            tx: value.tx,
            ty: value.ty,
        }
    }

    /// A translate or rotate transform can be added; one with scale or skew
    /// cannot, because adding scales would multiply them.
    pub fn is_add_safe(&self) -> bool {
        let d = self.decompose();
        (d.scale_x - 1.0).abs() < 1e-9 && (d.scale_y - 1.0).abs() < 1e-9 && d.skew.abs() < 1e-9
    }

    /// Component-wise accumulation with one-based scale and zero-based offsets.
    pub fn accumulate(&self, other: &Transform) -> Transform {
        let a = self.decompose();
        let b = other.decompose();
        Transform::from_decomposed(&Decomposed {
            tx: a.tx + b.tx,
            ty: a.ty + b.ty,
            rotation: a.rotation + b.rotation,
            scale_x: a.scale_x + b.scale_x - 1.0,
            scale_y: a.scale_y + b.scale_y - 1.0,
            skew: a.skew + b.skew,
        })
    }
}

impl Property for Transform {
    fn zero() -> Transform {
        Transform::ZERO
    }

    fn add_scaled(&mut self, other: &Transform, k: f64) {
        self.m11 += other.m11 * k;
        self.m12 += other.m12 * k;
        self.m21 += other.m21 * k;
        self.m22 += other.m22 * k;
        self.tx += other.tx * k;
        self.ty += other.ty * k;
    }

    fn lerp(&self, other: &Transform, t: f64) -> Transform {
        if t <= 0.0 {
            return *self;
        }
        if t >= 1.0 {
            return *other;
        }
        let a = self.decompose();
        let b = other.decompose();
        Transform::from_decomposed(&Decomposed {
            tx: a.tx + (b.tx - a.tx) * t,
            ty: a.ty + (b.ty - a.ty) * t,
            rotation: a.rotation + (b.rotation - a.rotation) * t,
            scale_x: a.scale_x + (b.scale_x - a.scale_x) * t,
            scale_y: a.scale_y + (b.scale_y - a.scale_y) * t,
            skew: a.skew + (b.skew - a.skew) * t,
        })
    }
}

/// The property types the crate evaluates directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Number(f64),
    Opacity(f32),
    Transform(Transform),
    Color(Color),
}

impl Value {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Number(_) => "number",
            Value::Opacity(_) => "opacity",
            Value::Transform(_) => "transform",
            Value::Color(_) => "color",
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_opacity(&self) -> Option<f32> {
        match self {
            Value::Opacity(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_transform(&self) -> Option<Transform> {
        match self {
            Value::Transform(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_color(&self) -> Option<Color> {
        match self {
            Value::Color(value) => Some(*value),
            _ => None,
        }
    }

    /// Clamp a value that declares a range. Opacity is clamped once, after the
    /// whole layer stack; the other types have no range.
    pub fn clamped(&self) -> Value {
        match self {
            Value::Opacity(value) => Value::Opacity(value.clamp(0.0, 1.0)),
            other => *other,
        }
    }
}

impl Property for Value {
    fn zero() -> Value {
        Value::Number(0.0)
    }

    fn add_scaled(&mut self, other: &Value, k: f64) {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => *a += *b * k,
            (Value::Opacity(a), Value::Opacity(b)) => *a += *b * k as f32,
            (Value::Transform(a), Value::Transform(b)) => a.add_scaled(b, k),
            (Value::Color(a), Value::Color(b)) => a.add_scaled(b, k),
            _ => {}
        }
    }

    fn lerp(&self, other: &Value, t: f64) -> Value {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => Value::Number(a.lerp(b, t)),
            (Value::Opacity(a), Value::Opacity(b)) => Value::Opacity(a.lerp(b, t)),
            (Value::Transform(a), Value::Transform(b)) => Value::Transform(a.lerp(b, t)),
            (Value::Color(a), Value::Color(b)) => Value::Color(a.lerp(b, t)),
            _ => *other,
        }
    }

    fn same_kind(&self, other: &Value) -> bool {
        core::mem::discriminant(self) == core::mem::discriminant(other)
    }

    fn scalar(&self) -> Option<f64> {
        match self {
            Value::Number(value) => Some(*value),
            Value::Opacity(value) => Some(*value as f64),
            _ => None,
        }
    }

    fn with_scalar(&self, value: f64) -> Option<Value> {
        match self {
            Value::Number(_) => Some(Value::Number(value)),
            Value::Opacity(_) => Some(Value::Opacity(value as f32)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::{Key, Track};

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn linear_blend_of_black_and_white_is_188_in_srgb() {
        let black = Color::srgb8(0, 0, 0, 255);
        let white = Color::srgb8(255, 255, 255, 255);
        let middle = black.lerp(&white, 0.5);
        let [r, g, b, a] = middle.to_srgb8();
        assert!((186..=190).contains(&r), "{r}");
        assert_eq!(r, g);
        assert_eq!(g, b);
        assert_eq!(a, 255);
    }

    #[test]
    fn premultiplied_alpha_survives_a_round_trip() {
        let colour = Color::srgb8(200, 100, 50, 128);
        let [r, g, b, a] = colour.to_srgb8();
        assert!((r as i32 - 200).abs() <= 1, "{r}");
        assert!((g as i32 - 100).abs() <= 1, "{g}");
        assert!((b as i32 - 50).abs() <= 1, "{b}");
        assert!((a as i32 - 128).abs() <= 1, "{a}");
    }

    #[test]
    fn transform_decomposition_round_trips() {
        let value = Transform::translate(12.0, -3.0)
            .mul(&Transform::rotate(0.7))
            .mul(&Transform::scale(2.0, 0.5));
        let back = Transform::from_decomposed(&value.decompose());
        assert!(close(back.m11, value.m11, 1e-9));
        assert!(close(back.m12, value.m12, 1e-9));
        assert!(close(back.m21, value.m21, 1e-9));
        assert!(close(back.m22, value.m22, 1e-9));
        assert!(close(back.tx, value.tx, 1e-9));
        assert!(close(back.ty, value.ty, 1e-9));
    }

    #[test]
    fn matrix_order_applies_the_right_hand_side_first() {
        let value =
            Transform::translate(10.0, 0.0).mul(&Transform::rotate(std::f64::consts::FRAC_PI_2));
        let [x, y] = value.apply(1.0, 0.0);
        assert!(close(x, 10.0, 1e-9));
        assert!(close(y, 1.0, 1e-9));
    }

    #[test]
    fn transform_lerp_is_exact_at_the_ends() {
        let a = Transform::translate(0.0, 0.0);
        let b = Transform::rotate(1.2).mul(&Transform::scale(2.0, 2.0));
        assert_eq!(a.lerp(&b, 0.0), a);
        assert_eq!(a.lerp(&b, 1.0), b);
        let middle = a.lerp(&b, 0.5);
        let d = middle.decompose();
        assert!(close(d.rotation, 0.6, 1e-9));
        assert!(close(d.scale_x, 1.5, 1e-9));
    }

    #[test]
    fn add_safety_follows_scale_and_skew() {
        assert!(Transform::translate(5.0, 2.0).is_add_safe());
        assert!(Transform::rotate(0.4).is_add_safe());
        assert!(!Transform::scale(2.0, 1.0).is_add_safe());
    }

    #[test]
    fn value_kinds_do_not_mix() {
        let number = Value::Number(1.0);
        let opacity = Value::Opacity(1.0);
        assert!(number.same_kind(&Value::Number(2.0)));
        assert!(!number.same_kind(&opacity));
        assert_eq!(number.with_scalar(3.0), Some(Value::Number(3.0)));
        assert_eq!(opacity.with_scalar(0.5), Some(Value::Opacity(0.5)));
        assert_eq!(Value::Color(Color::TRANSPARENT).with_scalar(0.5), None);
    }

    #[test]
    fn opacity_is_clamped_after_the_stack() {
        assert_eq!(Value::Opacity(1.4).clamped(), Value::Opacity(1.0));
        assert_eq!(Value::Opacity(-0.2).clamped(), Value::Opacity(0.0));
        assert_eq!(Value::Number(1.4).clamped(), Value::Number(1.4));
    }

    #[test]
    fn value_tracks_keep_their_kind() {
        let track = Track::new(vec![
            Key::new(0, Value::Opacity(0.0)),
            Key::new(1_000_000, Value::Opacity(1.0)),
        ]);
        let curve = track.prepare().unwrap();
        assert_eq!(curve.sample(500_000), Value::Opacity(0.5));
    }
}
