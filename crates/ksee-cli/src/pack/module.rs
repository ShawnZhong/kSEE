//! Reading the kernel's wasm module (wasmparser): its custom sections, its
//! DWARF and the plugin's ksee.code.

use std::collections::BTreeMap;

use wasmparser::{BinaryReaderError, Parser, Payload};

/// The custom sections of `bytes`, by name, in place.
pub fn custom_sections(bytes: &[u8]) -> Result<BTreeMap<&str, &[u8]>, BinaryReaderError> {
    let mut out = BTreeMap::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CustomSection(c) = payload? {
            out.insert(c.name(), c.data());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_modules_custom_sections() {
        let m = wat::parse_str(r#"(module (func) (@custom "ksee.code" "F") (@custom ".debug_info" "D"))"#).unwrap();
        let s = custom_sections(&m).unwrap();
        assert_eq!((s.get("ksee.code"), s.get(".debug_info")), (Some(&&b"F"[..]), Some(&&b"D"[..])));
    }
}
