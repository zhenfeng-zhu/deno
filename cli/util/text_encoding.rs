// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

use encoding_rs::*;
use std::borrow::Cow;
use std::io::Error;
use std::io::ErrorKind;

pub const BOM_CHAR: char = '\u{FEFF}';

/// Attempts to detect the character encoding of the provided bytes.
///
/// Supports UTF-8, UTF-16 Little Endian and UTF-16 Big Endian.
pub fn detect_charset(bytes: &'_ [u8]) -> &'static str {
  const UTF16_LE_BOM: &[u8] = b"\xFF\xFE";
  const UTF16_BE_BOM: &[u8] = b"\xFE\xFF";

  if bytes.starts_with(UTF16_LE_BOM) {
    "utf-16le"
  } else if bytes.starts_with(UTF16_BE_BOM) {
    "utf-16be"
  } else {
    // Assume everything else is utf-8
    "utf-8"
  }
}

/// Attempts to convert the provided bytes to a UTF-8 string.
///
/// Supports all encodings supported by the encoding_rs crate, which includes
/// all encodings specified in the WHATWG Encoding Standard, and only those
/// encodings (see: <https://encoding.spec.whatwg.org/>).
pub fn convert_to_utf8<'a>(
  bytes: &'a [u8],
  charset: &'_ str,
) -> Result<Cow<'a, str>, Error> {
  match Encoding::for_label(charset.as_bytes()) {
    Some(encoding) => encoding
      .decode_without_bom_handling_and_without_replacement(bytes)
      .ok_or_else(|| ErrorKind::InvalidData.into()),
    None => Err(Error::new(
      ErrorKind::InvalidInput,
      format!("Unsupported charset: {}", charset),
    )),
  }
}

/// Strips the byte order mark from the provided text if it exists.
pub fn strip_bom(text: &str) -> &str {
  if text.starts_with(BOM_CHAR) {
    &text[BOM_CHAR.len_utf8()..]
  } else {
    text
  }
}

static SOURCE_MAP_PREFIX: &str =
  "//# sourceMappingURL=data:application/json;base64,";

pub fn source_map_from_code(code: &str) -> Option<Vec<u8>> {
  let last_line = code.rsplit(|u| u == '\n').next()?;
  if last_line.starts_with(SOURCE_MAP_PREFIX) {
    let input = last_line.split_at(SOURCE_MAP_PREFIX.len()).1;
    let decoded_map = base64::decode(input)
      .expect("Unable to decode source map from emitted file.");
    Some(decoded_map)
  } else {
    None
  }
}

pub fn code_without_source_map(mut code: String) -> String {
  if let Some(last_line_index) = code.rfind('\n') {
    if code[last_line_index + 1..].starts_with(SOURCE_MAP_PREFIX) {
      code.truncate(last_line_index + 1);
      code
    } else {
      code
    }
  } else {
    code
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn test_detection(test_data: &[u8], expected_charset: &str) {
    let detected_charset = detect_charset(test_data);
    assert_eq!(
      expected_charset.to_lowercase(),
      detected_charset.to_lowercase()
    );
  }

  #[test]
  fn test_detection_utf8_no_bom() {
    let test_data = "Hello UTF-8 it is \u{23F0} for Deno!"
      .to_owned()
      .into_bytes();
    test_detection(&test_data, "utf-8");
  }

  #[test]
  fn test_detection_utf16_little_endian() {
    let test_data = b"\xFF\xFEHello UTF-16LE".to_owned().to_vec();
    test_detection(&test_data, "utf-16le");
  }

  #[test]
  fn test_detection_utf16_big_endian() {
    let test_data = b"\xFE\xFFHello UTF-16BE".to_owned().to_vec();
    test_detection(&test_data, "utf-16be");
  }

  #[test]
  fn test_decoding_unsupported_charset() {
    let test_data = Vec::new();
    let result = convert_to_utf8(&test_data, "utf-32le");
    assert!(result.is_err());
    let err = result.expect_err("Err expected");
    assert!(err.kind() == ErrorKind::InvalidInput);
  }

  #[test]
  fn test_decoding_invalid_utf8() {
    let test_data = b"\xFE\xFE\xFF\xFF".to_vec();
    let result = convert_to_utf8(&test_data, "utf-8");
    assert!(result.is_err());
    let err = result.expect_err("Err expected");
    assert!(err.kind() == ErrorKind::InvalidData);
  }

  #[test]
  fn test_source_without_source_map() {
    run_test("", "");
    run_test("\n", "\n");
    run_test("\r\n", "\r\n");
    run_test("a", "a");
    run_test("a\n", "a\n");
    run_test("a\r\n", "a\r\n");
    run_test("a\r\nb", "a\r\nb");
    run_test("a\nb\n", "a\nb\n");
    run_test("a\r\nb\r\n", "a\r\nb\r\n");
    run_test(
      "test\n//# sourceMappingURL=data:application/json;base64,test",
      "test\n",
    );
    run_test(
      "test\r\n//# sourceMappingURL=data:application/json;base64,test",
      "test\r\n",
    );
    run_test(
      "\n//# sourceMappingURL=data:application/json;base64,test",
      "\n",
    );

    fn run_test(input: &str, output: &str) {
      assert_eq!(code_without_source_map(input.to_string()), output);
    }
  }
}
