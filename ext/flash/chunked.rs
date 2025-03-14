// Based on https://github.com/frewsxcv/rust-chunked-transfer/blob/5c08614458580f9e7a85124021006d83ce1ed6e9/src/decoder.rs
// Copyright 2015 The tiny-http Contributors
// Copyright 2015 The rust-chunked-transfer Contributors
// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

use std::error::Error;
use std::fmt;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Result as IoResult;

pub struct Decoder<R> {
  pub source: R,

  // remaining size of the chunk being read
  // none if we are not in a chunk
  pub remaining_chunks_size: Option<usize>,
  pub end: bool,
}

impl<R> Decoder<R>
where
  R: Read,
{
  pub fn new(source: R, remaining_chunks_size: Option<usize>) -> Decoder<R> {
    Decoder {
      source,
      remaining_chunks_size,
      end: false,
    }
  }

  fn read_chunk_size(&mut self) -> IoResult<usize> {
    let mut chunk_size_bytes = Vec::new();
    let mut has_ext = false;

    loop {
      let byte = match self.source.by_ref().bytes().next() {
        Some(b) => b?,
        None => {
          return Err(IoError::new(ErrorKind::InvalidInput, DecoderError))
        }
      };

      if byte == b'\r' {
        break;
      }

      if byte == b';' {
        has_ext = true;
        break;
      }

      chunk_size_bytes.push(byte);
    }

    // Ignore extensions for now
    if has_ext {
      loop {
        let byte = match self.source.by_ref().bytes().next() {
          Some(b) => b?,
          None => {
            return Err(IoError::new(ErrorKind::InvalidInput, DecoderError))
          }
        };
        if byte == b'\r' {
          break;
        }
      }
    }

    self.read_line_feed()?;

    let chunk_size = String::from_utf8(chunk_size_bytes)
      .ok()
      .and_then(|c| usize::from_str_radix(c.trim(), 16).ok())
      .ok_or_else(|| IoError::new(ErrorKind::InvalidInput, DecoderError))?;

    Ok(chunk_size)
  }

  fn read_carriage_return(&mut self) -> IoResult<()> {
    match self.source.by_ref().bytes().next() {
      Some(Ok(b'\r')) => Ok(()),
      _ => Err(IoError::new(ErrorKind::InvalidInput, DecoderError)),
    }
  }

  fn read_line_feed(&mut self) -> IoResult<()> {
    match self.source.by_ref().bytes().next() {
      Some(Ok(b'\n')) => Ok(()),
      _ => Err(IoError::new(ErrorKind::InvalidInput, DecoderError)),
    }
  }
}

impl<R> Read for Decoder<R>
where
  R: Read,
{
  fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
    let remaining_chunks_size = match self.remaining_chunks_size {
      Some(c) => c,
      None => {
        // first possibility: we are not in a chunk, so we'll attempt to determine
        // the chunks size
        let chunk_size = self.read_chunk_size()?;

        // if the chunk size is 0, we are at EOF
        if chunk_size == 0 {
          self.read_carriage_return()?;
          self.read_line_feed()?;
          self.end = true;
          return Ok(0);
        }

        chunk_size
      }
    };

    // second possibility: we continue reading from a chunk
    if buf.len() < remaining_chunks_size {
      let read = self.source.read(buf)?;
      self.remaining_chunks_size = Some(remaining_chunks_size - read);
      return Ok(read);
    }

    // third possibility: the read request goes further than the current chunk
    // we simply read until the end of the chunk and return
    let buf = &mut buf[..remaining_chunks_size];
    let read = self.source.read(buf)?;
    self.remaining_chunks_size = if read == remaining_chunks_size {
      self.read_carriage_return()?;
      self.read_line_feed()?;
      None
    } else {
      Some(remaining_chunks_size - read)
    };

    Ok(read)
  }
}

#[derive(Debug, Copy, Clone)]
struct DecoderError;

impl fmt::Display for DecoderError {
  fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
    write!(fmt, "Error while decoding chunks")
  }
}

impl Error for DecoderError {
  fn description(&self) -> &str {
    "Error while decoding chunks"
  }
}

#[cfg(test)]
mod test {
  use super::Decoder;
  use std::io;
  use std::io::Read;

  /// This unit test is taken from from Hyper
  /// https://github.com/hyperium/hyper
  /// Copyright (c) 2014 Sean McArthur
  #[test]
  fn test_read_chunk_size() {
    fn read(s: &str, expected: usize) {
      let mut decoded = Decoder::new(s.as_bytes(), None);
      let actual = decoded.read_chunk_size().unwrap();
      assert_eq!(expected, actual);
    }

    fn read_err(s: &str) {
      let mut decoded = Decoder::new(s.as_bytes(), None);
      let err_kind = decoded.read_chunk_size().unwrap_err().kind();
      assert_eq!(err_kind, io::ErrorKind::InvalidInput);
    }

    read("1\r\n", 1);
    read("01\r\n", 1);
    read("0\r\n", 0);
    read("00\r\n", 0);
    read("A\r\n", 10);
    read("a\r\n", 10);
    read("Ff\r\n", 255);
    read("Ff   \r\n", 255);
    // Missing LF or CRLF
    read_err("F\rF");
    read_err("F");
    // Invalid hex digit
    read_err("X\r\n");
    read_err("1X\r\n");
    read_err("-\r\n");
    read_err("-1\r\n");
    // Acceptable (if not fully valid) extensions do not influence the size
    read("1;extension\r\n", 1);
    read("a;ext name=value\r\n", 10);
    read("1;extension;extension2\r\n", 1);
    read("1;;;  ;\r\n", 1);
    read("2; extension...\r\n", 2);
    read("3   ; extension=123\r\n", 3);
    read("3   ;\r\n", 3);
    read("3   ;   \r\n", 3);
    // Invalid extensions cause an error
    read_err("1 invalid extension\r\n");
    read_err("1 A\r\n");
    read_err("1;no CRLF");
  }

  #[test]
  fn test_valid_chunk_decode() {
    let source = io::Cursor::new(
      "3\r\nhel\r\nb\r\nlo world!!!\r\n0\r\n\r\n"
        .to_string()
        .into_bytes(),
    );
    let mut decoded = Decoder::new(source, None);

    let mut string = String::new();
    decoded.read_to_string(&mut string).unwrap();

    assert_eq!(string, "hello world!!!");
  }

  #[test]
  fn test_decode_zero_length() {
    let mut decoder = Decoder::new(b"0\r\n\r\n" as &[u8], None);

    let mut decoded = String::new();
    decoder.read_to_string(&mut decoded).unwrap();

    assert_eq!(decoded, "");
  }

  #[test]
  fn test_decode_invalid_chunk_length() {
    let mut decoder = Decoder::new(b"m\r\n\r\n" as &[u8], None);

    let mut decoded = String::new();
    assert!(decoder.read_to_string(&mut decoded).is_err());
  }

  #[test]
  fn invalid_input1() {
    let source = io::Cursor::new(
      "2\r\nhel\r\nb\r\nlo world!!!\r\n0\r\n"
        .to_string()
        .into_bytes(),
    );
    let mut decoded = Decoder::new(source, None);

    let mut string = String::new();
    assert!(decoded.read_to_string(&mut string).is_err());
  }

  #[test]
  fn invalid_input2() {
    let source = io::Cursor::new(
      "3\rhel\r\nb\r\nlo world!!!\r\n0\r\n"
        .to_string()
        .into_bytes(),
    );
    let mut decoded = Decoder::new(source, None);

    let mut string = String::new();
    assert!(decoded.read_to_string(&mut string).is_err());
  }
}
