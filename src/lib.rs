use js_sys::{ArrayBuffer, Uint8Array};
use std::rc::Rc;
use std::string::FromUtf8Error;

#[derive(Debug)]
pub struct Blob {
    data: Rc<[Vec<u8>]>,
    opts: BlobOptions,
    view: Option<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub struct BlobOptions {
    endings: LineEndings,
    ty: Option<Box<str>>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LineEndings {
    /// Convert newlines to the host system's native convention.
    Native,

    /// Copies newline characters into the blob without changing them.
    Transparent,
}

fn normalize_line_endings(input: &str) -> Option<String> {
    // The end offset of the last char that was read into the output buffer.
    let mut offset = 0;

    // An iterator over the byte position of every occurance of `\n` in input.
    #[cfg(target_os = "windows")]
    let mut indices = input.match_indices("\n").peekable();

    // An iterator over the byte position of every occurance of `\r\n` in input.
    #[cfg(not(target_os = "windows"))]
    let mut indices = input.match_indices("\r\n").peekable();

    // Conditionally preallocate an owned string to use as an output buffer.
    let mut normalized = match indices.peek() {
        // The input string requires line ending normalization. Preallocate a
        // string with as much capacity as the input.
        Some(_) => String::with_capacity(input.len()),

        // The input string does not require line ending normalization. Return
        // early.
        None => return None,
    };

    for (start, le) in &mut indices {
        // A slice of the input string from the index of the last char that was
        // read into the normalized output buffer to the start of the line
        // ending that requires substitution.
        let slice = &input[offset..start];

        // Append the slice to buffer from text. Starting from the end offset
        // of the previous line ending that we replaced to the start of the
        // line ending that we're currently replacing.
        normalized.push_str(slice);

        // On windows, conditionally prepend an `\r` before any occurance of a
        // `\n`. This maintains compatibility with protocols that use `\r` as
        // a control char (such as SMTP).
        #[cfg(target_os = "windows")]
        if !slice.ends_with('\r') {
            normalized.push('\r');
        }

        // Unconditionally append the `\n` char.
        normalized.push('\n');

        // Advance the offset pointer to the start index of the char that
        // immediately follows the line ending that was replaced.
        offset = start + le.len();
    }

    // Append the remaining slice of the input to the output buffer. The bounds
    // check is necessary in cases where the input string ends with a line ending
    // that needed replaced.
    if let Some(remaining) = input.get(offset..) {
        normalized.push_str(remaining);
    }

    Some(normalized)
}

impl Blob {
    /// Constructs a new Blob instance with the provided data
    ///
    #[inline]
    pub fn new<I, A>(parts: I, opts: Option<BlobOptions>) -> Self
    where
        Vec<u8>: From<A>,
        I: IntoIterator<Item = A>,
    {
        Self {
            data: parts.into_iter().map(Vec::from).collect(),
            opts: opts.unwrap_or_default(),
            view: None,
        }
    }

    /// Create a new Blob instance from `start` to `end` and an optional
    /// Content-Type argument.
    ///
    /// If the `end` is `None`, the original length of the underlying buffer will
    /// be used instead.
    ///
    pub fn slice(&self, start: usize, end: Option<usize>, ty: Option<String>) -> Self {
        // Store the optionally Content-Type string as a Box<str> to lower the
        // memory footprint of BlobOptions.
        let ty = ty.map(Box::from);

        // If `end` is `None` use `self.size()` to get the index of the last byte
        // that will be contained in the newly returned Blob.
        let end = end.unwrap_or_else(|| self.size());

        Self {
            data: Rc::clone(&self.data),
            opts: BlobOptions::new(LineEndings::Transparent, ty),
            view: Some((start, end)),
        }
    }

    /// The size of the underlying buffer in bytes.
    ///
    #[inline]
    pub fn size(&self) -> usize {
        match self.view {
            // Get the length of the view by subtracting the end index from the
            // start index. If overflow occurs, panic. In practice, we would want
            // to branch on various deployment targets (i.e Node, Deno, Browsers)
            // and throw a RangeError.
            Some((from, to)) => to - from,

            // Get the length by calculating the sum of each part.
            None => self.data.iter().map(|part| part.len()).sum(),
        }
    }

    /// Returns an optional reference to the Content-Type string of the data
    /// stored in self.
    ///
    #[inline]
    pub fn ty(&self) -> Option<&str> {
        match &self.opts.ty {
            Some(ty) => Some(ty),
            None => None,
        }
    }

    /// An immmutable view of the underlying buffer.
    ///
    pub async fn array_buffer(&self) -> ArrayBuffer {
        self.coalesce_js().buffer()
    }

    /// Returns a `Future` that resolves to a byte slice.
    ///
    pub async fn bytes(&self) -> Uint8Array {
        self.coalesce_js()
    }

    /// Returns a `ReadableStream` that can be used in a browser.
    ///
    pub async fn stream() {
        todo!("integrate with web-sys and return an actual ReadableStream")
    }

    /// Returns a `Future` that resolves to a &str.
    ///
    /// # Errors
    ///
    /// If the data stored in the Blob's buffer contains an invalid UTF-8 code
    /// sequence.
    ///
    pub async fn text(&self) -> Result<String, FromUtf8Error> {
        // Validate that the bytes stored in self.data is valid UTF-8 sequence.
        let text = String::from_utf8(self.coalesce())?;

        if self.opts.endings == LineEndings::Native {
            Ok(normalize_line_endings(&text).unwrap_or(text))
        } else {
            Ok(text)
        }
    }
}

impl Blob {
    fn coalesce(&self) -> Vec<u8> {
        // Calculate the length of the buffer we are creating from self.
        let capacity = self.size();

        // If we are working with a Blob slice, use the range stored at
        // self.view. Otherwise, use 0 for the start index and `len` for the end.
        let (from, to) = self.view.unwrap_or((0, capacity));

        // Allocate a zero-filled buffer with the total length
        // of the view we are creating from self.
        let mut buffer = vec![0; capacity];

        // The absolute index of the byte that will be read into the output
        // buffer.
        let mut abs = 0;

        let mut ptr = 0;

        // Iterate over each part of the blob.
        for part in self.data.iter() {
            let len = part.len();
            let edge = abs + len;

            // Determine if the start index is stored in part.
            if from > edge {
                abs = edge;
                continue;
            }

            for byte in part.iter() {
                abs += 1;

                if from >= abs {
                    continue;
                }

                // If the offset pointer is greater than our end index, return.
                if abs > to {
                    return buffer;
                }

                // Set the value at ptr to byte.
                buffer[ptr] = *byte;
                // Increment the offset pointer.
                ptr += 1;
            }
        }

        buffer
    }

    fn coalesce_js(&self) -> Uint8Array {
        // Calculate the length of the buffer we are creating from self.
        let len = self.size();

        // If we are working with a Blob slice, use the range stored at
        // self.view. Otherwise, use 0 for the start index and `len` for the end.
        let (from, to) = self.view.unwrap_or((0, len));

        // Allocate a zero-filled buffer with the total length
        // of the view we are creating from self.
        //
        // TODO: determine what to do in the case of an overflow.
        let buffer = Uint8Array::new_with_length(len as u32);

        // The absolute index of the byte that will be read into the output
        // buffer.
        let mut abs = 0;

        let mut ptr = 0;

        // Iterate over each part of the blob.
        for part in self.data.iter() {
            let edge = abs + part.len();

            // Determine if the start index is stored in part.
            if from > edge {
                abs = edge;
                continue;
            }

            for byte in part.iter() {
                abs += 1;

                if from >= abs {
                    continue;
                }

                // If the offset pointer is greater than our end index, return.
                if abs > to {
                    return buffer;
                }

                // Set the value at ptr to byte.
                buffer.set_index(ptr, *byte);

                // Increment the offset pointer.
                ptr += 1;
            }
        }

        buffer
    }
}

impl BlobOptions {
    #[inline]
    pub fn new(endings: LineEndings, ty: Option<Box<str>>) -> Self {
        Self { endings, ty }
    }
}

impl Default for BlobOptions {
    #[inline]
    fn default() -> Self {
        Self::new(LineEndings::Transparent, None)
    }
}

#[cfg(test)]
mod tests {
    use super::{Blob, BlobOptions, LineEndings};

    const DATA: &[u8] = b"First line\r\nSecond line\nThird line\r\nFourth line";

    //
    // TODO: figure out how to test with js-sys and setup wasm-bindgen.
    //
    // #[tokio::test]
    // async fn bytes() {
    //     let blob = Blob::new(vec![DATA.to_vec()], None);
    //     let bytes = blob.bytes().await;

    //     for (index, byte) in DATA.iter().enumerate() {
    //         assert_eq!(bytes.get_index(index as u32), *byte);
    //     }
    // }

    #[tokio::test]
    async fn multipart() {
        let blob = Blob::new(vec![DATA, DATA], None);
        let mut data = DATA.to_vec();

        data.extend_from_slice(DATA);

        assert_eq!(blob.text().await.unwrap().as_bytes(), data);
    }

    #[tokio::test]
    async fn slice() {
        let blob = Blob::new(vec![DATA.to_vec()], None);
        let slice = blob.slice(12, Some(23), None);

        assert_eq!(slice.text().await.unwrap(), "Second line");
    }

    #[tokio::test]
    async fn text_native() {
        let blob = Blob::new(
            vec![DATA.to_vec()],
            Some(BlobOptions::new(LineEndings::Native, None)),
        );

        #[cfg(target_os = "windows")]
        assert_eq!(
            blob.text().await.unwrap(),
            "First line\r\nSecond line\r\nThird line\r\nFourth line"
        );

        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            blob.text().await.unwrap(),
            "First line\nSecond line\nThird line\nFourth line"
        );
    }

    #[tokio::test]
    async fn text_transparent() {
        let blob = Blob::new(vec![DATA.to_vec()], None);
        assert_eq!(blob.text().await.unwrap().as_bytes(), DATA);
    }
}
