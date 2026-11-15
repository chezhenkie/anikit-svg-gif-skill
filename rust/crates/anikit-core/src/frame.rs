//! A plain RGB8 frame: the only pixel type the core knows.

use crate::{Error, Result};

/// An image as packed RGB8 rows, top down, no padding.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Frame {
    /// A black frame of the given size.
    pub fn new(width: u32, height: u32) -> Result<Frame> {
        if width == 0 || height == 0 {
            return Err(Error::invalid("frame size must be greater than zero"));
        }
        let len = width as usize * height as usize * 3;
        Ok(Frame {
            width,
            height,
            data: vec![0; len],
        })
    }

    /// A frame from an existing buffer. The length must be exactly w * h * 3.
    pub fn from_rgb(width: u32, height: u32, data: Vec<u8>) -> Result<Frame> {
        if width == 0 || height == 0 {
            return Err(Error::invalid("frame size must be greater than zero"));
        }
        let want = width as usize * height as usize * 3;
        if data.len() != want {
            return Err(Error::invalid(format!(
                "frame buffer is {} bytes, expected {} for {}x{}",
                data.len(),
                want,
                width,
                height
            )));
        }
        Ok(Frame {
            width,
            height,
            data,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn pixels(&self) -> usize {
        self.width as usize * self.height as usize
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    pub fn set_pixel(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        self.data[i..i + 3].copy_from_slice(&rgb);
    }

    /// Fill every pixel with one colour.
    pub fn fill(&mut self, rgb: [u8; 3]) {
        let (pixels, _) = self.data.as_chunks_mut::<3>();
        for pixel in pixels {
            *pixel = rgb;
        }
    }

    /// True when both frames have the same size and identical bytes.
    pub fn same_as(&self, other: &Frame) -> bool {
        self.width == other.width && self.height == other.height && self.data == other.data
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame {}x{}", self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_length_is_checked() {
        assert!(Frame::from_rgb(2, 2, vec![0; 11]).is_err());
        assert!(Frame::from_rgb(2, 2, vec![0; 12]).is_ok());
    }

    #[test]
    fn zero_size_is_rejected() {
        assert!(Frame::new(0, 4).is_err());
        assert!(Frame::new(4, 0).is_err());
    }

    #[test]
    fn pixel_round_trip() {
        let mut frame = Frame::new(3, 2).unwrap();
        frame.set_pixel(2, 1, [1, 2, 3]);
        assert_eq!(frame.pixel(2, 1), [1, 2, 3]);
        assert_eq!(frame.pixel(0, 0), [0, 0, 0]);
    }
}
