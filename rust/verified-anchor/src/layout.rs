//! Byte-level Borsh field locator. Mirrors the Lean `Solana/Borsh/Locate.lean` model
//! function-for-function; the pair is the Rust<->Lean correspondence this milestone rests on.
//! Deliberately does NOT call the `borsh` crate: agreement with it is established empirically
//! by `tests/borsh_differential.rs`, not assumed.

use solana_program::pubkey::Pubkey;

/// Borsh type descriptor. `&'static` recursion keeps the whole tree constructible in a `const`,
/// which is what lets `#[derive(AccountData)]` emit it as an associated const.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    U8, U16, U32, U64, U128,
    I8, I16, I32, I64, I128,
    Bool,
    Pubkey,
    Array(&'static Ty, usize),
    String,
    Vec(&'static Ty),
    Option(&'static Ty),
    Struct(&'static [(&'static str, Ty)]),
}

/// A decoded scalar. Mirrors the Lean `Value`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value<'a> {
    Nat(u128),
    Int(i128),
    Bool(bool),
    Key(Pubkey),
    Bytes(&'a [u8]),
}

/// A field of a DESERIALISED account struct, read as the same `Value` the byte-level
/// `read_val` would have produced from the same field.
///
/// WHY THIS EXISTS (M10 Task 13). `#[derive(AccountData)]` truncates the Borsh descriptor at
/// the first field whose type it cannot map, so `constraint = vault.amount >= 1000` over
/// `struct NameVault { name: [u8; N], amount: u64 }` (`N` a named const, still unmappable —
/// M10 Task 15b) names a field the descriptor does not record. `locate` would return `None`
/// there and the constraint would reject EVERY account.
/// The macro therefore const-selects: locatable => the proven byte-level check in `validate`;
/// not locatable => the SAME expression re-evaluated in `try_accounts` against the
/// deserialised struct, through this trait.
///
/// The impls mirror `read_val` ARM FOR ARM, including its refusals: an aggregate yields `None`
/// there, so it yields `None` here, and a constraint reading one fails closed on both paths.
/// The fallback is unproven — Lean cannot model a read the descriptor does not describe — but
/// it is the same SEMANTICS, which is what keeps the two paths from disagreeing about a
/// program that moves between them when a field type becomes mappable.
pub trait FieldValue {
    fn field_value(&self) -> Option<Value<'static>>;
}

macro_rules! field_value_nat {
    ($($t:ty),*) => { $(
        impl FieldValue for $t {
            fn field_value(&self) -> Option<Value<'static>> { Some(Value::Nat(*self as u128)) }
        }
    )* };
}
macro_rules! field_value_int {
    ($($t:ty),*) => { $(
        impl FieldValue for $t {
            fn field_value(&self) -> Option<Value<'static>> { Some(Value::Int(*self as i128)) }
        }
    )* };
}
field_value_nat!(u8, u16, u32, u64, u128);
field_value_int!(i8, i16, i32, i64, i128);

impl FieldValue for bool {
    fn field_value(&self) -> Option<Value<'static>> { Some(Value::Bool(*self)) }
}
impl FieldValue for Pubkey {
    fn field_value(&self) -> Option<Value<'static>> { Some(Value::Key(*self)) }
}
// The aggregates `read_val` refuses. Present so a constraint over one still COMPILES (real
// Anchor accepts it) and still fails closed, exactly as the byte-level path does.
impl<T> FieldValue for Option<T> {
    fn field_value(&self) -> Option<Value<'static>> { None }
}
impl<T> FieldValue for Vec<T> {
    fn field_value(&self) -> Option<Value<'static>> { None }
}
impl FieldValue for String {
    fn field_value(&self) -> Option<Value<'static>> { None }
}
impl<T, const N: usize> FieldValue for [T; N] {
    fn field_value(&self) -> Option<Value<'static>> { None }
}

impl Ty {
    /// Fixed encoded width, or `None` for variable-length types. Mirrors Lean `Ty.byteSize`.
    pub const fn byte_size(&self) -> Option<usize> {
        match self {
            Ty::U8 | Ty::I8 | Ty::Bool => Some(1),
            Ty::U16 | Ty::I16 => Some(2),
            Ty::U32 | Ty::I32 => Some(4),
            Ty::U64 | Ty::I64 => Some(8),
            Ty::U128 | Ty::I128 => Some(16),
            Ty::Pubkey => Some(32),
            Ty::Array(e, n) => match e.byte_size() {
                Some(w) => Some(w * *n),
                None => None,
            },
            Ty::String | Ty::Vec(_) | Ty::Option(_) => None,
            Ty::Struct(fs) => {
                // const fn: manual loop, no iterators
                let mut acc = 0usize;
                let mut i = 0usize;
                while i < fs.len() {
                    match fs[i].1.byte_size() {
                        Some(w) => acc += w,
                        None => return None,
                    }
                    i += 1;
                }
                Some(acc)
            }
        }
    }
}

/// Little-endian unsigned read of `w` bytes at `off`. `None` if out of bounds.
/// Mirrors Lean `readUIntLE`.
fn read_uint_le(data: &[u8], off: usize, w: usize) -> Option<u128> {
    let end = off.checked_add(w)?;
    if end > data.len() {
        return None;
    }
    let mut acc: u128 = 0;
    for i in 0..w {
        acc |= (data[off + i] as u128) << (8 * i);
    }
    Some(acc)
}

/// Encoded width of the value starting at `off`. Unlike `Ty::byte_size`, which is a static
/// property of the type, this consults the *bytes*: a `string`/`vec` width is only knowable
/// after reading its u32 length prefix, and an `option`'s only after its 1-byte tag. This is
/// what lets `locate` step over a variable-length field to reach the one behind it.
///
/// Mirrors Lean `encodedWidth` extensionally. Lean's `.vec` arm is a structurally-recursive
/// loop (`vecWidthFrom`, lifted out for kernel-reducibility reasons that don't apply to Rust);
/// here it is inlined directly. For a FIXED-width element the loop is additionally short-
/// circuited into a single multiply (`4 + count * w`) rather than iterated `count` times: an
/// attacker controls the u32 count directly, so looping per-element is an unbounded-compute
/// footgun on-chain (a count near 4e9 would burn the whole CU budget before the subsequent
/// `end <= data.len()` bounds check ever gets a chance to fail closed). This is extensionally
/// identical to the Lean loop (same value for every input) — no divergence for the Tasks
/// 6/10/11 agreement proofs — and it does NOT early-return or cap the count: an absurd count
/// still fails closed later, in `locate`'s/`read_val`'s bounds checks, exactly as Lean does.
/// Variable-width elements keep looping (they already terminate early on a short buffer,
/// because a truncated encoding makes the per-element `encoded_width` call return `None`).
pub fn encoded_width(ty: &Ty, data: &[u8], off: usize) -> Option<usize> {
    match ty {
        Ty::String => {
            let n = read_uint_le(data, off, 4)? as usize;
            let end = off.checked_add(4)?.checked_add(n)?;
            if end <= data.len() { Some(4 + n) } else { None }
        }
        Ty::Vec(e) => {
            let count = read_uint_le(data, off, 4)? as usize;
            let start = off.checked_add(4)?;
            if let Some(w) = e.byte_size() {
                // Fixed-width element: closed-form width, no per-element loop (see doc comment).
                let total = count.checked_mul(w)?;
                return Some(4usize.checked_add(total)?);
            }
            let mut cur = start;
            let mut acc = 4usize;
            for _ in 0..count {
                let w = encoded_width(e, data, cur)?;
                cur = cur.checked_add(w)?;
                acc = acc.checked_add(w)?;
            }
            Some(acc)
        }
        Ty::Option(e) => {
            let tag = read_uint_le(data, off, 1)?;
            if tag == 0 {
                Some(1)
            } else {
                Some(1 + encoded_width(e, data, off + 1)?)
            }
        }
        Ty::Struct(fs) => {
            let mut cur = off;
            let mut acc = 0usize;
            for (_, fty) in fs.iter() {
                let w = encoded_width(fty, data, cur)?;
                cur = cur.checked_add(w)?;
                acc = acc.checked_add(w)?;
            }
            Some(acc)
        }
        other => other.byte_size(),
    }
}

/// Walk `path` through `ty` from byte offset `start`. Mirrors Lean `locate`.
///
/// NOTE: like Lean, this may legitimately return an offset PAST the end of `data` (e.g. a
/// trailing field of a struct whose buffer was truncated) — `encoded_width` computing a width
/// is not the same claim as the bytes existing. Real bounds safety lives in `read_val`. Callers
/// must never treat "`locate` returned `Some`" as "the field is actually present"; do not add a
/// bounds check here that Lean's `locate` lacks, or the two definitions diverge.
pub fn locate(ty: &Ty, path: &[&str], data: &[u8], start: usize) -> Option<(usize, Ty)> {
    let (name, rest) = match path.split_first() {
        None => return Some((start, *ty)),
        Some(x) => x,
    };
    let fs = match ty {
        Ty::Struct(fs) => fs,
        _ => return None,
    };
    let mut cur = start;
    for (fname, fty) in fs.iter() {
        if fname == name {
            return locate(fty, rest, data, cur);
        }
        let w = encoded_width(fty, data, cur)?;
        cur = cur.checked_add(w)?;
    }
    None
}

/// Decode one scalar at `off`. Mirrors Lean `readVal`. Aggregates yield `None` — `.array` of a
/// variable-width element is included in that: Lean's `readVal` has no `.array` arm that
/// inspects the element type, it fails closed for every `.array` unconditionally, and this
/// mirrors that (fail-closed, not "sometimes read the array").
pub fn read_val<'a>(ty: &Ty, data: &'a [u8], off: usize) -> Option<Value<'a>> {
    match ty {
        Ty::U8 => read_uint_le(data, off, 1).map(Value::Nat),
        Ty::U16 => read_uint_le(data, off, 2).map(Value::Nat),
        Ty::U32 => read_uint_le(data, off, 4).map(Value::Nat),
        Ty::U64 => read_uint_le(data, off, 8).map(Value::Nat),
        Ty::U128 => read_uint_le(data, off, 16).map(Value::Nat),
        // Two's-complement fixup via widen-then-narrow-then-widen: `n as u128 as i8` truncates
        // to the low byte and reinterprets its top bit as sign, which is exactly Lean's
        // `if n < 2^(w*8-1) then n else n - 2^(w*8)` (verified at the boundaries in tests below).
        Ty::I8 => read_uint_le(data, off, 1).map(|n| Value::Int(n as i8 as i128)),
        Ty::I16 => read_uint_le(data, off, 2).map(|n| Value::Int(n as i16 as i128)),
        Ty::I32 => read_uint_le(data, off, 4).map(|n| Value::Int(n as i32 as i128)),
        Ty::I64 => read_uint_le(data, off, 8).map(|n| Value::Int(n as i64 as i128)),
        Ty::I128 => read_uint_le(data, off, 16).map(|n| Value::Int(n as i128)),
        Ty::Bool => read_uint_le(data, off, 1).map(|n| Value::Bool(n != 0)),
        Ty::Pubkey => {
            let end = off.checked_add(32)?;
            if end > data.len() {
                return None;
            }
            let mut b = [0u8; 32];
            b.copy_from_slice(&data[off..end]);
            Some(Value::Key(Pubkey::new_from_array(b)))
        }
        // `.array` of a variable-width element yields `None` in Lean (fail-closed), and so does
        // every other aggregate; mirrored unconditionally rather than special-cased.
        Ty::Array(_, _) | Ty::String | Ty::Vec(_) | Ty::Option(_) | Ty::Struct(_) => None,
    }
}

/// `==` over `&str` in a `const` context. `str::eq` is not `const`, and `PartialEq` cannot be
/// called from const code at all, so the byte compare is spelled out.
const fn const_str_eq(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The `Ty` of `name` among `ty`'s TOP-LEVEL fields, or `None` if there is no such field.
///
/// Deliberately const-evaluable, and deliberately NOT a second implementation of `locate`:
/// `locate` consults the account BYTES (it must, to step over variable-width fields), so it can
/// only ever run at runtime. This answers the strictly weaker, data-independent question
/// "could `locate` ever find this field?", which is decidable from the descriptor alone. That
/// is what lets `#[derive(VerifiedAccounts)]` turn an unlocatable `has_one` target into a BUILD
/// error instead of an instruction that rejects every legitimate account at runtime.
///
/// Why an unlocatable target is reachable from correct-looking code: `map_ty` does not cover
/// nested structs or enums, and (M10 Task 15b) only maps a fixed-size array whose length is an
/// integer LITERAL — a const-generic or named-const length is still unevaluable at macro time.
/// `#[derive(AccountData)]` truncates the descriptor at the first field it cannot map
/// (everything after it has an unknowable offset). So a perfectly valid Anchor struct whose
/// `has_one` target sits behind, say, a `[u8; N]` with `N` a named const yields a descriptor
/// that simply does not mention the target.
pub const fn top_level_field(ty: Ty, name: &str) -> Option<Ty> {
    let fs = match ty {
        Ty::Struct(fs) => fs,
        _ => return None,
    };
    // const fn: manual loop, no iterators
    let mut i = 0;
    while i < fs.len() {
        if const_str_eq(fs[i].0, name) {
            return Some(fs[i].1);
        }
        i += 1;
    }
    None
}

/// True when `name` is a top-level field of `ty` — i.e. `locate(ty, &[name], .., ..)` has a
/// chance of succeeding. See `top_level_field`.
pub const fn has_top_level_field(ty: Ty, name: &str) -> bool {
    top_level_field(ty, name).is_some()
}

/// True when `name` is a top-level field of `ty` AND is a `Pubkey`. `has_one` compares the
/// stored value against an account key, so any other type makes `read_val` yield a non-`Key`
/// `Value` and the check reject unconditionally — another silent runtime brick, caught at build
/// time by the same mechanism.
pub const fn has_top_level_pubkey_field(ty: Ty, name: &str) -> bool {
    matches!(top_level_field(ty, name), Some(Ty::Pubkey))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VAULT: Ty = Ty::Struct(&[("bump", Ty::U8), ("authority", Ty::Pubkey)]);
    const NAMED: Ty = Ty::Struct(&[("label", Ty::String), ("owner", Ty::Pubkey)]);

    fn vault_bytes() -> Vec<u8> {
        let mut v = vec![0u8; 8];
        v.push(7);
        v.extend_from_slice(&[3u8; 32]);
        v
    }

    fn named_bytes() -> Vec<u8> {
        let mut v = vec![0u8; 8];
        v.extend_from_slice(&2u32.to_le_bytes());
        v.extend_from_slice(b"hi");
        v.extend_from_slice(&[9u8; 32]);
        v
    }

    #[test]
    fn locates_fixed_fields() {
        let d = vault_bytes();
        assert_eq!(locate(&VAULT, &["bump"], &d, 8).map(|r| r.0), Some(8));
        assert_eq!(locate(&VAULT, &["authority"], &d, 8).map(|r| r.0), Some(9));
    }

    #[test]
    fn unknown_field_is_none() {
        let d = vault_bytes();
        assert_eq!(locate(&VAULT, &["missing"], &d, 8), None);
    }

    /// The whole point: a field after a variable-length String is still locatable.
    #[test]
    fn locates_field_after_string() {
        let d = named_bytes();
        assert_eq!(locate(&NAMED, &["owner"], &d, 8).map(|r| r.0), Some(14));
    }

    #[test]
    fn reads_scalars_little_endian() {
        assert_eq!(read_val(&Ty::U8, &vault_bytes(), 8), Some(Value::Nat(7)));
        assert_eq!(read_val(&Ty::U64, &[1, 0, 0, 0, 0, 0, 0, 0], 0), Some(Value::Nat(1)));
        assert_eq!(read_val(&Ty::U64, &[0, 1, 0, 0, 0, 0, 0, 0], 0), Some(Value::Nat(256)));
        assert_eq!(read_val(&Ty::Bool, &[0], 0), Some(Value::Bool(false)));
        assert_eq!(read_val(&Ty::Bool, &[1], 0), Some(Value::Bool(true)));
    }

    #[test]
    fn out_of_bounds_fails_closed() {
        assert_eq!(read_val(&Ty::U64, &[1, 2, 3], 0), None);
        // Corrected from the task brief's literal text, which asserted `None` here. `locate`
        // mirrors Lean (verified by hand-tracing `Locate.lean`'s `locate`/`locateInFields`/
        // `encodedWidth` on this exact input), and Lean does NOT bounds-check while skipping
        // fixed-width fields: `bump`'s width is static (`Ty.byteSize`) and never consults
        // `data`, so the walk correctly reaches offset 9 for `authority` even though this
        // 8-byte buffer holds none of `authority`'s bytes and not even all of `bump`'s own
        // byte. This is carried-forward finding #2 (an invariant to PRESERVE, not a bug):
        // `locate` may return an offset past the end of the buffer; real bounds safety lives
        // in `read_val`, as the assertion above demonstrates. See also
        // `locate_may_return_past_end_of_buffer`. Adding a bounds check here to make this
        // assert `None` would diverge from Lean and break the Tasks 6/10/11 agreement proofs.
        assert_eq!(locate(&VAULT, &["authority"], &[0u8; 8], 8), Some((9, Ty::Pubkey)));
    }

    /// Carried-forward Task-2 finding: no fixture in either language exercised the signed
    /// boundary values yet. Bytes are produced with `to_le_bytes` (std, independently correct
    /// two's complement) and checked against Lean's `if n < 2^(w*8-1) then n else n - 2^(w*8)`
    /// formula, worked by hand for the `-1` cases in the comments below.
    #[test]
    fn signed_values_match_twos_complement() {
        // -1 in every width is all-0xFF bytes. Lean: n = 2^(w*8) - 1 >= 2^(w*8-1), so
        // int = n - 2^(w*8) = -1. Matches for every width.
        assert_eq!(read_val(&Ty::I8, &(-1i8).to_le_bytes(), 0), Some(Value::Int(-1)));
        assert_eq!(read_val(&Ty::I16, &(-1i16).to_le_bytes(), 0), Some(Value::Int(-1)));
        assert_eq!(read_val(&Ty::I32, &(-1i32).to_le_bytes(), 0), Some(Value::Int(-1)));
        assert_eq!(read_val(&Ty::I64, &(-1i64).to_le_bytes(), 0), Some(Value::Int(-1)));
        assert_eq!(read_val(&Ty::I128, &(-1i128).to_le_bytes(), 0), Some(Value::Int(-1)));

        // 0 in every width: n = 0 < 2^(w*8-1), so int = n = 0 unconditionally.
        assert_eq!(read_val(&Ty::I8, &0i8.to_le_bytes(), 0), Some(Value::Int(0)));
        assert_eq!(read_val(&Ty::I16, &0i16.to_le_bytes(), 0), Some(Value::Int(0)));
        assert_eq!(read_val(&Ty::I32, &0i32.to_le_bytes(), 0), Some(Value::Int(0)));
        assert_eq!(read_val(&Ty::I64, &0i64.to_le_bytes(), 0), Some(Value::Int(0)));
        assert_eq!(read_val(&Ty::I128, &0i128.to_le_bytes(), 0), Some(Value::Int(0)));

        // i64::MIN: n = 2^63 (the top bit alone), which is NOT < 2^63, so
        // int = 2^63 - 2^64 = -2^63 = i64::MIN. This is the boundary where a naive
        // `n < 2^(w*8-1)` off-by-one would misfire.
        assert_eq!(
            read_val(&Ty::I64, &i64::MIN.to_le_bytes(), 0),
            Some(Value::Int(i64::MIN as i128))
        );
        // i64::MAX: n = 2^63 - 1 < 2^63, so int = n = 2^63 - 1 = i64::MAX.
        assert_eq!(
            read_val(&Ty::I64, &i64::MAX.to_le_bytes(), 0),
            Some(Value::Int(i64::MAX as i128))
        );
    }

    /// Carried-forward Task-2 finding #1: a `Vec` of fixed-width elements must not loop over
    /// an attacker-controlled count. Assert the closed-form answer matches what the (safe, small)
    /// loop would have produced, and separately that the loop is not what's being hit (a
    /// huge declared count with a short buffer still resolves in O(1), not by iterating).
    #[test]
    fn vec_of_fixed_width_elements_is_not_a_loop_over_count() {
        // 3 u8 elements: count=3, then 3 payload bytes.
        let d = [3u8, 0, 0, 0, 1, 2, 3];
        assert_eq!(encoded_width(&Ty::Vec(&Ty::U8), &d, 0), Some(4 + 3));

        // A huge declared count (u32::MAX) with a fixed-width (8-byte) element and a short
        // buffer: the closed-form multiply overflows a smaller int but not usize/u128 checked
        // arithmetic, and the result correctly exceeds the buffer, but must NOT be computed by
        // looping 4 billion times (this test would time out if it were).
        let d2 = (u32::MAX).to_le_bytes();
        assert_eq!(encoded_width(&Ty::Vec(&Ty::U64), &d2, 0), Some(4 + (u32::MAX as usize) * 8));

        // Variable-width elements still loop (and fail closed on a short buffer).
        let d3 = 5u32.to_le_bytes(); // count = 5, but zero payload bytes follow
        assert_eq!(encoded_width(&Ty::Vec(&Ty::String), &d3, 0), None);
    }

    /// Carried-forward Task-2 finding #4: `.array` of a variable-width element fails closed,
    /// mirroring Lean's unconditional `.array _ _ => none` arm in `readVal`.
    #[test]
    fn array_read_val_is_always_none() {
        assert_eq!(read_val(&Ty::Array(&Ty::U8, 4), &[1, 2, 3, 4], 0), None);
        assert_eq!(read_val(&Ty::Array(&Ty::String, 2), &[0u8; 16], 0), None);
    }

    /// Carried-forward Task-2 finding #2: `locate` may return an offset past the end of the
    /// buffer — that is not itself an error, and callers must go through `read_val` (or an
    /// explicit bounds check) to learn whether the field is actually present.
    #[test]
    fn locate_may_return_past_end_of_buffer() {
        // VAULT.authority is a fixed 32 bytes at offset 1 (after `bump`). `locate` only needs
        // the widths of PRECEDING fields — bump's u8 width is static, no bytes consulted — so
        // it happily returns Some(1) even for a completely empty buffer, none of authority's
        // 32 bytes present.
        let empty: &[u8] = &[];
        assert_eq!(locate(&VAULT, &["authority"], empty, 0).map(|r| r.0), Some(1));
        // The offset is real but the bytes are not: read_val on that offset must fail closed.
        assert_eq!(read_val(&Ty::Pubkey, empty, 1), None);
    }
}

#[cfg(test)]
mod const_lookup_tests {
    use super::*;

    const VAULT: Ty = Ty::Struct(&[("bump", Ty::U8), ("authority", Ty::Pubkey)]);
    // What `#[derive(AccountData)]` emits for `struct NameVault { name: [u8; N], authority:
    // Pubkey }` with `N` a named const: `map_ty` (M10 Task 15b) only maps a LITERAL array
    // length, so the descriptor TRUNCATES at `name` and never mentions `authority`. This is
    // the shape the build-time `has_one` guard must reject.
    const TRUNCATED: Ty = Ty::Struct(&[]);

    // Evaluated by the compiler, not the test runner: these are the exact expressions the
    // generated `const _: () = assert!(..)` relies on being const-evaluable at all.
    const _: () = assert!(has_top_level_field(VAULT, "authority"));
    const _: () = assert!(has_top_level_pubkey_field(VAULT, "authority"));
    const _: () = assert!(!has_top_level_field(TRUNCATED, "authority"));

    #[test]
    fn finds_a_top_level_field_and_its_ty() {
        assert_eq!(top_level_field(VAULT, "bump"), Some(Ty::U8));
        assert_eq!(top_level_field(VAULT, "authority"), Some(Ty::Pubkey));
    }

    #[test]
    fn misses_are_none() {
        assert_eq!(top_level_field(VAULT, "nope"), None);
        // A truncated descriptor is exactly how a legitimate struct loses its has_one target.
        assert_eq!(top_level_field(TRUNCATED, "authority"), None);
        // Non-struct descriptors have no named fields at all.
        assert_eq!(top_level_field(Ty::Pubkey, "authority"), None);
    }

    #[test]
    fn pubkey_predicate_rejects_present_but_wrongly_typed_targets() {
        assert!(has_top_level_field(VAULT, "bump"));
        assert!(!has_top_level_pubkey_field(VAULT, "bump"));
    }

    #[test]
    fn const_str_eq_matches_the_runtime_operator() {
        for (a, b) in [("a", "a"), ("a", "b"), ("", ""), ("ab", "a"), ("a", "ab"), ("ab", "ab")] {
            assert_eq!(const_str_eq(a, b), a == b, "{a:?} vs {b:?}");
        }
    }

    /// The const predicate must agree with the runtime `locate` it is guarding: whenever it says
    /// "absent", `locate` must genuinely fail for every buffer, not just some.
    #[test]
    fn agrees_with_locate() {
        let data = vec![0u8; 64];
        for name in ["bump", "authority", "nope"] {
            assert_eq!(
                has_top_level_field(VAULT, name),
                locate(&VAULT, &[name], &data, 8).is_some(),
                "disagreement on {name:?}"
            );
        }
        assert!(locate(&TRUNCATED, &["authority"], &data, 8).is_none());
    }
}
