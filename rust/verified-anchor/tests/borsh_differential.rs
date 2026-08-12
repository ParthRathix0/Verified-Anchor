//! Differential cross-check: our locator vs the real `borsh` crate's encoding.
//! This is the empirical half of the M10 trust boundary — the Lean model and the Rust
//! locator mirror each other by construction, but only these tests tie either to the
//! encoding `borsh` actually emits.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;
use verified_anchor::layout::{locate, read_val, Ty, Value};

/// Every field fixed-size, target NOT first — the shape v0.3.0 mis-checks.
#[derive(BorshSerialize, BorshDeserialize)]
struct Fixed {
    bump: u8,
    count: u64,
    authority: Pubkey,
    flag: bool,
}

const FIXED_TY: Ty = Ty::Struct(&[
    ("bump", Ty::U8),
    ("count", Ty::U64),
    ("authority", Ty::Pubkey),
    ("flag", Ty::Bool),
]);

/// A variable-length prefix, so `owner` has no static offset.
#[derive(BorshSerialize, BorshDeserialize)]
struct WithString {
    label: String,
    owner: Pubkey,
    amount: u64,
}

const WITH_STRING_TY: Ty = Ty::Struct(&[
    ("label", Ty::String),
    ("owner", Ty::Pubkey),
    ("amount", Ty::U64),
]);

#[derive(BorshSerialize, BorshDeserialize)]
struct WithVecOption {
    xs: Vec<u32>,
    maybe: Option<u64>,
    tail: Pubkey,
}

const WITH_VEC_OPTION_TY: Ty = Ty::Struct(&[
    ("xs", Ty::Vec(&Ty::U32)),
    ("maybe", Ty::Option(&Ty::U64)),
    ("tail", Ty::Pubkey),
]);

/// A struct nested inside another struct. Borsh has no special encoding for nesting — the
/// inner struct's fields are simply serialized in place — so this exercises `locate`'s
/// recursive descent through a multi-segment `path` (`["inner", "b"]`), not a new wire form.
#[derive(BorshSerialize, BorshDeserialize)]
struct Inner {
    a: u16,
    b: Pubkey,
}

#[derive(BorshSerialize, BorshDeserialize)]
struct Outer {
    inner: Inner,
    extra: u32,
}

const INNER_TY: Ty = Ty::Struct(&[("a", Ty::U16), ("b", Ty::Pubkey)]);
const OUTER_TY: Ty = Ty::Struct(&[("inner", INNER_TY), ("extra", Ty::U32)]);

/// Carried gap (Task 3 note): no fixture in either language round-tripped signed types
/// through the real `borsh` crate. `i8`..`i128`, including `i64::MIN`/`i64::MAX` and an
/// `i128` boundary, are covered by `signed_integers_match_borsh_encoding` below.
#[derive(BorshSerialize, BorshDeserialize)]
struct Signed {
    a: i8,
    b: i16,
    c: i32,
    d: i64,
    e: i128,
    tail: Pubkey,
}

const SIGNED_TY: Ty = Ty::Struct(&[
    ("a", Ty::I8),
    ("b", Ty::I16),
    ("c", Ty::I32),
    ("d", Ty::I64),
    ("e", Ty::I128),
    ("tail", Ty::Pubkey),
]);

fn key_at(ty: &Ty, path: &[&str], data: &[u8]) -> Pubkey {
    let (off, fty) = locate(ty, path, data, 0).expect("locate failed");
    match read_val(&fty, data, off).expect("read failed") {
        Value::Key(k) => k,
        v => panic!("expected key, got {v:?}"),
    }
}

fn nat_at(ty: &Ty, path: &[&str], data: &[u8]) -> u128 {
    let (off, fty) = locate(ty, path, data, 0).expect("locate failed");
    match read_val(&fty, data, off).expect("read failed") {
        Value::Nat(n) => n,
        v => panic!("expected nat, got {v:?}"),
    }
}

fn int_at(ty: &Ty, path: &[&str], data: &[u8]) -> i128 {
    let (off, fty) = locate(ty, path, data, 0).expect("locate failed");
    match read_val(&fty, data, off).expect("read failed") {
        Value::Int(n) => n,
        v => panic!("expected int, got {v:?}"),
    }
}

#[test]
fn fixed_layout_matches_borsh() {
    let auth = Pubkey::new_unique();
    let s = Fixed { bump: 254, count: 4242, authority: auth, flag: true };
    let bytes = borsh::to_vec(&s).unwrap();

    assert_eq!(nat_at(&FIXED_TY, &["bump"], &bytes), 254);
    assert_eq!(nat_at(&FIXED_TY, &["count"], &bytes), 4242);
    assert_eq!(key_at(&FIXED_TY, &["authority"], &bytes), auth);

    let (off, fty) = locate(&FIXED_TY, &["flag"], &bytes, 0).unwrap();
    assert_eq!(read_val(&fty, &bytes, off), Some(Value::Bool(true)));
}

#[test]
fn string_prefixed_layout_matches_borsh() {
    let owner = Pubkey::new_unique();
    for label in ["", "a", "a much longer label value"] {
        let s = WithString { label: label.to_string(), owner, amount: 7 };
        let bytes = borsh::to_vec(&s).unwrap();
        assert_eq!(key_at(&WITH_STRING_TY, &["owner"], &bytes), owner, "label={label:?}");
        assert_eq!(nat_at(&WITH_STRING_TY, &["amount"], &bytes), 7, "label={label:?}");
    }
}

#[test]
fn vec_and_option_layout_matches_borsh() {
    let tail = Pubkey::new_unique();
    for (xs, maybe) in [
        (vec![], None),
        (vec![1u32], Some(9u64)),
        (vec![1u32, 2, 3, 4], None),
        (vec![7u32; 20], Some(u64::MAX)),
    ] {
        let s = WithVecOption { xs: xs.clone(), maybe, tail };
        let bytes = borsh::to_vec(&s).unwrap();
        assert_eq!(
            key_at(&WITH_VEC_OPTION_TY, &["tail"], &bytes),
            tail,
            "xs={xs:?} maybe={maybe:?}"
        );
    }
}

/// The locator must agree with what the crate's own deserializer reads back.
#[test]
fn locator_agrees_with_deserialized_value() {
    let owner = Pubkey::new_unique();
    let s = WithString { label: "vault-one".into(), owner, amount: 123456789 };
    let bytes = borsh::to_vec(&s).unwrap();
    let back = WithString::try_from_slice(&bytes).unwrap();

    assert_eq!(key_at(&WITH_STRING_TY, &["owner"], &bytes), back.owner);
    assert_eq!(nat_at(&WITH_STRING_TY, &["amount"], &bytes), back.amount as u128);
}

/// Truncated buffers must fail closed, never panic and never read adjacent memory.
#[test]
fn truncated_buffers_fail_closed() {
    let s = WithString { label: "xy".into(), owner: Pubkey::new_unique(), amount: 1 };
    let bytes = borsh::to_vec(&s).unwrap();
    // Anchor the "fail closed" checks below in a decoded-value check too, so this test still
    // depends on `read_uint_le` actually being little-endian (an all-truncation test would
    // pass even against a byte-order bug, since it never inspects a decoded value).
    assert_eq!(nat_at(&WITH_STRING_TY, &["amount"], &bytes), 1);
    for cut in 0..bytes.len() {
        let _ = locate(&WITH_STRING_TY, &["amount"], &bytes[..cut], 0);
    }
    // Cutting to 3 bytes truncates even `label`'s own 4-byte u32 length prefix, so
    // `encoded_width` cannot skip past `label` at all — this is a case that genuinely
    // consults `data` and must fail closed (unlike a fixed-width prefix, which would let
    // `locate` return an offset past the end of the buffer; see `layout.rs`'s
    // `locate_may_return_past_end_of_buffer`).
    assert_eq!(locate(&WITH_STRING_TY, &["amount"], &bytes[..3], 0), None);
}

/// Nesting: `locate` must walk a multi-segment path through a struct-valued field the same
/// way `borsh` lays out a nested struct — inline, with no extra tag or length prefix.
#[test]
fn nested_struct_layout_matches_borsh() {
    let b = Pubkey::new_unique();
    let s = Outer { inner: Inner { a: 999, b }, extra: 0xDEADBEEF };
    let bytes = borsh::to_vec(&s).unwrap();

    assert_eq!(nat_at(&OUTER_TY, &["inner", "a"], &bytes), 999);
    assert_eq!(key_at(&OUTER_TY, &["inner", "b"], &bytes), b);
    assert_eq!(nat_at(&OUTER_TY, &["extra"], &bytes), 0xDEADBEEF);
}

/// Carried gap (Task 3): signed types had no fixture that went through the real `borsh`
/// crate. Covers every signed width, negative values, and the `i64` boundary — the point
/// where a naive `n < 2^(w*8-1)` sign-fixup would misfire.
#[test]
fn signed_integers_match_borsh_encoding() {
    let tail = Pubkey::new_unique();
    let cases: [(i8, i16, i32, i64, i128); 6] = [
        (0, 0, 0, 0, 0),
        (-1, -1, -1, -1, -1),
        (i8::MAX, i16::MAX, i32::MAX, i64::MAX, i128::MAX),
        (i8::MIN, i16::MIN, i32::MIN, i64::MIN, i128::MIN),
        (-42, -1234, -123_456, i64::MIN, i128::MIN),
        (42, 1234, 123_456, i64::MAX, i128::MAX),
    ];
    for (a, b, c, d, e) in cases {
        let s = Signed { a, b, c, d, e, tail };
        let bytes = borsh::to_vec(&s).unwrap();

        assert_eq!(int_at(&SIGNED_TY, &["a"], &bytes), a as i128, "a={a}");
        assert_eq!(int_at(&SIGNED_TY, &["b"], &bytes), b as i128, "b={b}");
        assert_eq!(int_at(&SIGNED_TY, &["c"], &bytes), c as i128, "c={c}");
        assert_eq!(int_at(&SIGNED_TY, &["d"], &bytes), d as i128, "d={d}");
        assert_eq!(int_at(&SIGNED_TY, &["e"], &bytes), e, "e={e}");
        assert_eq!(key_at(&SIGNED_TY, &["tail"], &bytes), tail);
    }
}

/// M10 Task 15b: `map_ty` gained a `[T; N]` arm so fixed-size arrays no longer truncate
/// `#[derive(AccountData)]`'s descriptor. `Ty::Array`'s `byte_size`/`encoded_width` predate
/// that change and were never exercised against the real crate — Borsh encodes a fixed array
/// with NO length prefix (unlike `Vec`, which is length-prefixed), so a locator that assumed
/// otherwise would still pass every test that only inspects our own model. Two arrays back to
/// back (`[u8; 32]` then `[u64; 4]`, 32 + 32 = 64 bytes with no framing between or after them)
/// so a one-array fixture could not hide an off-by-one at the boundary.
#[derive(BorshSerialize, BorshDeserialize)]
struct WithArrays {
    root: [u8; 32],
    scores: [u64; 4],
    tail: Pubkey,
}

const WITH_ARRAYS_TY: Ty = Ty::Struct(&[
    ("root", Ty::Array(&Ty::U8, 32)),
    ("scores", Ty::Array(&Ty::U64, 4)),
    ("tail", Ty::Pubkey),
]);

#[test]
fn fixed_arrays_layout_matches_borsh() {
    let root = [7u8; 32];
    let scores = [1u64, 2, 3, 4];
    let tail = Pubkey::new_unique();
    let s = WithArrays { root, scores, tail };
    let bytes = borsh::to_vec(&s).unwrap();

    // `borsh::to_vec` is the ground truth for the wire size: no length prefix on either
    // array means the whole struct is exactly 32 + 32 + 32 = 96 bytes.
    assert_eq!(bytes.len(), 96, "borsh's own encoding grew a prefix this test didn't expect");

    // the field positioned AFTER both arrays must be locatable, and at the byte offset borsh
    // actually placed it at (64), not wherever a length-prefixed assumption would predict.
    let (off, fty) = locate(&WITH_ARRAYS_TY, &["tail"], &bytes, 0).expect("locate failed");
    assert_eq!(off, 64);
    assert_eq!(key_at(&WITH_ARRAYS_TY, &["tail"], &bytes), tail);
    let _ = fty;

    // and the deserialised value must round-trip through the real crate to the same `tail`,
    // tying this to `borsh`'s own reader, not just our own assumption about its writer.
    let back = WithArrays::try_from_slice(&bytes).unwrap();
    assert_eq!(back.tail, tail);
    assert_eq!(back.root, root);
    assert_eq!(back.scores, scores);
}
