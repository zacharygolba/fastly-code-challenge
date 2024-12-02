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
    // We use this store the end offset of the last char that was read into
    // buffer.
    let mut n_read = 0;

    // Prefer a single allocation over the possbility of 0 or n allocations.
    // This will be slightly slower for strings that do not require line ending
    // normalization but is likely on average, better than conditionally
    // allocating in a loop.
    let mut output = String::with_capacity(input.len());

    // On windows, we conditionally prepend an `\r` before any occurance of
    // a `\n`. This maintains compatibility with protocols that use `\r` as a
    // control char (such as SMTP).
    #[cfg(target_os = "windows")]
    let endings = input.match_indices("\n");

    // On all other platforms, we simply replace `\r\n` with `\n`.
    #[cfg(not(target_os = "windows"))]
    let endings = input.match_indices("\r\n");

    for (start, pat) in endings {
        let prefix = &input[n_read..start];

        // Append the slice to buffer from text. Starting from the end offset
        // of the previous line ending that we replaced to the start of the
        // line ending that we're currently replacing.
        output.push_str(prefix);

        #[cfg(target_os = "windows")]
        if !prefix.ends_with('\r') {
            output.push('\r');
        }

        // Unconditionally append the `\n` char.
        output.push('\n');

        // Update the offset to include the length of prefix and the
        // matched pattern.
        n_read = start + pat.len();
    }

    if n_read == 0 {
        // We didn't have to replace any line endings. We can return a reference
        // rather than an owned version of the input string.
        return Cow::Borrowed(input);
    }

    if let Some(remaining) = input.get(n_read..) {
        // Append the remaining suffix of input to our output buffer.
        output.push_str(remaining);
    }

    // Return an owned version of the input data with normalized line endings.
    Cow::Owned(output)
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
        self.opts.ty.as_ref().map(|ty| &**ty)
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
        // Integrate with js-sys to return an actuall ArrayBuffer that
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
    pub fn text(&self) -> Result<Cow<str>, Utf8Error> {
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
        assert!(from < to, "index out of bounds");
        assert!(from >= self.from, "start index out of bounds");
        assert!(to <= self.to, "end index out of bounds");

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

    #[test]
    fn line_endings() {
        let blob = Blob::new(
            DATA.to_vec(),
            Some(BlobOptions::new(LineEndings::Native, None)),
        );

        #[cfg(target_os = "windows")]
        assert_eq!(
            blob.text().unwrap(),
            "First line\r\nSecond line\r\nThird line\r\nFourth line"
        );

        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            blob.text().unwrap(),
            "First line\nSecond line\nThird line\nFourth line"
        );
    }

    #[test]
    fn slice() {
        let blob = Blob::new(DATA.to_vec(), None);
        let slice = blob.slice(12, Some(23), None);

        assert_eq!(slice.text().unwrap(), "Second line");
    }

    #[test]
    fn text() {
        let blob = Blob::new(DATA.to_vec(), None);

        assert_eq!(blob.text().unwrap().as_bytes(), DATA);
    }
}
