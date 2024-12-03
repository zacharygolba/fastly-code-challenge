use std::borrow::Cow;
use std::rc::Rc;
use std::str::{self, Utf8Error};

#[derive(Debug, Clone)]
pub struct Blob {
    data: BlobSlice,
    opts: BlobOptions,
}

#[derive(Debug, Clone)]
pub struct BlobSlice {
    buffer: Rc<[u8]>,
    from: usize,
    to: usize,
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

fn normalize_line_endings(input: &str) -> Cow<str> {
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
        None => return Cow::Borrowed(input),
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

    // Return an owned version of the input string with normalized line endings.
    Cow::Owned(normalized)
}

impl Blob {
    /// Constructs a new Blob instance with the provided data
    ///
    #[inline]
    pub fn new(data: Vec<u8>, opts: Option<BlobOptions>) -> Self {
        Self {
            data: data.into(),
            opts: opts.unwrap_or_default(),
        }
    }

    /// Create a new Blob instance from `start` to `end` and an optional
    /// Content-Type argument.
    ///
    /// If the `end` is `None`, the original length of the underlying buffer will
    /// be used instead.
    ///
    pub fn slice(&self, start: usize, end: Option<usize>, ty: Option<String>) -> Self {
        // If `end` is `None` use `self.size()` to get the index of last byte
        // that will be contained in the newly returned Blob.
        let end = end.unwrap_or_else(|| self.size());

        // Convert the provided Content-Type string to a Box<str> and wrap it
        // in a BlobOptions.
        let opts = BlobOptions::new(LineEndings::Transparent, ty.map(Box::from));

        // Get a reference to bytes that the newly returned Blob will contain.
        // A relatively easy and future optimization that can be made to
        // prevent duplicating data could swapping `Vec<u8>` for `Rc<[u8]>`
        // in single-threaded environments or `Arc<[u8]>` in multi-threaded
        // environments. For now, copying the data is the most pratical choice.
        let data = self.data.slice(start, end);

        Self { data, opts }
    }

    /// The size of the underlying buffer in bytes.
    ///
    #[inline]
    pub fn size(&self) -> usize {
        self.data.len()
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
    pub fn array_buffer(&self) -> &[u8] {
        // TODO:
        //
        // Use js_sys or similar to return an actual WASM compatible
        // ArrayBuffer object.
        self.data.as_slice()
    }

    /// Returns a `Future` that resolves to a byte slice.
    ///
    pub async fn bytes(&self) -> &[u8] {
        //
        // TODO:
        //
        // Integrate with js-sys to return an actual ArrayBuffer that
        // can be used in a browser.
        //
        self.data.as_slice()
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
    pub async fn text(&self) -> Result<Cow<str>, Utf8Error> {
        // Validate that the bytes stored in self.data is valid UTF-8 sequence.
        let text = str::from_utf8(self.data.as_slice())?;

        if self.opts.endings == LineEndings::Native {
            Ok(normalize_line_endings(&text))
        } else {
            Ok(Cow::Borrowed(text))
        }
    }
}

impl BlobSlice {
    fn slice(&self, from: usize, to: usize) -> Self {
        // Pardon the sloppy bounds checks.
        assert!(from >= self.from, "start index out of bounds");
        assert!(to <= self.to, "end index out of bounds");
        assert!(from < to, "slice range out of bounds");

        Self {
            buffer: Rc::clone(&self.buffer),
            from,
            to,
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.buffer[self.from..self.to]
    }

    fn len(&self) -> usize {
        self.to - self.from
    }
}

impl From<Vec<u8>> for BlobSlice {
    fn from(buffer: Vec<u8>) -> Self {
        let len = buffer.len();

        Self {
            buffer: buffer.into(),
            from: 0,
            to: len,
        }
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

    #[tokio::test]
    async fn slice() {
        let blob = Blob::new(DATA.to_vec(), None);
        let slice = blob.slice(12, Some(23), None);

        assert_eq!(slice.text().await.unwrap(), "Second line");
    }

    #[tokio::test]
    async fn text_native() {
        let blob = Blob::new(
            DATA.to_vec(),
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
        let blob = Blob::new(DATA.to_vec(), None);
        assert_eq!(blob.text().await.unwrap().as_bytes(), DATA);
    }
}
