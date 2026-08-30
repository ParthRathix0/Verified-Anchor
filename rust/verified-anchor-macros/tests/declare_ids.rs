use std::fs;
use std::path::Path;

const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn base58_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut bytes: Vec<u8> = vec![0];
    for c in input.chars() {
        let digit = ALPHABET
            .iter()
            .position(|&b| b as char == c)
            .ok_or_else(|| format!("illegal base58 char {c:?}"))?;
        let mut carry = digit as u32;
        for byte in bytes.iter_mut() {
            carry += *byte as u32 * 58;
            *byte = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push(carry as u8);
            carry >>= 8;
        }
    }
    // leading '1's are leading zero bytes, not zero-value digits
    let leading_zeros = input.chars().take_while(|&c| c == '1').count();
    bytes.resize(bytes.len() + leading_zeros, 0);
    bytes.reverse();
    Ok(bytes)
}

fn declare_ids_in(path: &Path, out: &mut Vec<(String, String)>) {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {path:?}: {e}"));
    let needle = "declare_id!(\"";
    let mut rest = text.as_str();
    while let Some(start) = rest.find(needle) {
        rest = &rest[start + needle.len()..];
        let end = rest
            .find('"')
            .unwrap_or_else(|| panic!("unterminated declare_id! literal in {path:?}"));
        out.push((path.display().to_string(), rest[..end].to_string()));
        rest = &rest[end..];
    }
}

fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("reading dir {dir:?}: {e}")) {
        let path = entry
            .unwrap_or_else(|e| panic!("reading entry in {dir:?}: {e}"))
            .path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            declare_ids_in(&path, out);
        }
    }
}

#[test]
fn every_ui_fixture_declare_id_is_a_valid_pubkey() {
    let ui_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui");
    let mut ids = Vec::new();
    walk(&ui_dir, &mut ids);

    assert!(
        !ids.is_empty(),
        "found no declare_id! literals under {ui_dir:?} — did the fixtures move?"
    );

    for (file, id) in &ids {
        let decoded = base58_decode(id)
            .unwrap_or_else(|e| panic!("{file}: {id:?} is not valid base58 ({e})"));
        assert_eq!(
            decoded.len(),
            32,
            "{file}: {id:?} decodes to {} bytes, expected 32",
            decoded.len()
        );
    }
}
