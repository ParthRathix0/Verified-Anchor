//! Compiles a `syn::Expr` from `#[account(constraint = ...)]` into the proven relational
//! sublanguage. Returning `None` is NOT an error — it routes the expression to the escape
//! hatch (see `lib.rs`), which runs it verbatim as Rust. Never emit a `compile_error!` from
//! this module: real Anchor accepts arbitrary expressions and so must we.
//!
//! THE INVARIANT THIS FILE EXISTS TO UPHOLD: the emitted Rust must agree with Lean's
//! `evalExpr` (`lean/VerifiedAnchor/Constraints/Expr.lean`) on EVERY input, and in particular
//! must never ACCEPT where Lean yields `none`. `evalExpr` is written in the `Option` monad and
//! is STRICT in `and`/`or` — both operands are bound before they are combined — so the codegen
//! below computes an `Option<bool>` per operand, combines them strictly, and treats `None` as
//! rejection. See the `and`/`or` arms of `to_tokens` for the full argument; the short version
//! is that Rust's native `||` would short-circuit `true || <unevaluable>` to `true` (ACCEPT)
//! where the contract says `none` (REJECT), which is exactly the gap the milestone's headline
//! guarantee forbids.

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{BinOp, Expr, UnOp};

pub(crate) enum Cmp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Cmp {
    fn lean(&self) -> &'static str {
        match self {
            Cmp::Eq => "Cmp.eq",
            Cmp::Ne => "Cmp.ne",
            Cmp::Lt => "Cmp.lt",
            Cmp::Le => "Cmp.le",
            Cmp::Gt => "Cmp.gt",
            Cmp::Ge => "Cmp.ge",
        }
    }
}

pub(crate) enum Operand {
    LitNat(u128),
    /// A NEGATED integer literal. Split from `LitNat` because Lean's `Value` distinguishes
    /// `.nat` from `.int`, and only `.int` can carry a negative number at all. Since M10's
    /// `evalCmp` widening the two compare NUMERICALLY against each other, so this split is now
    /// about representability, not about which comparisons are answerable.
    LitInt(i128),
    LitBool(bool),
    Field(usize, Vec<String>),
    Key(usize),
    Owner(usize),
    Lamports(usize),
    DataLen(usize),
    IsSigner(usize),
    IsWritable(usize),
    Executable(usize),
    InstrArg(String),
}

pub(crate) enum VExpr {
    Cmp(Cmp, Operand, Operand),
    And(Box<VExpr>, Box<VExpr>),
    Or(Box<VExpr>, Box<VExpr>),
    Not(Box<VExpr>),
    Truthy(Operand),
}

/// What the compiler needs to resolve names: field name -> index, the typed inner type of
/// each `Account<'info, T>` field, and the declared instruction-argument names.
pub(crate) struct ExprCtx<'a> {
    pub index_of: &'a std::collections::HashMap<String, usize>,
    pub inner_ty: &'a std::collections::HashMap<String, syn::Type>,
    pub instr_args: &'a [String],
}

// ── Parsing: `syn::Expr` -> `VExpr` ───────────────────────────────────────────────────────

fn operand(e: &Expr, ctx: &ExprCtx) -> Option<Operand> {
    match e {
        Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => {
            Some(Operand::LitNat(i.base10_parse::<u128>().ok()?))
        }
        Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(b), .. }) => Some(Operand::LitBool(b.value)),
        // `-1` is `Unary(Neg, Lit(1))`, not a negative literal token.
        Expr::Unary(syn::ExprUnary { op: UnOp::Neg(_), expr, .. }) => match expr.as_ref() {
            Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => {
                let n = i.base10_parse::<i128>().ok()?;
                Some(Operand::LitInt(n.checked_neg()?))
            }
            _ => None,
        },
        // bare identifier: an instruction argument
        Expr::Path(p) => {
            let id = p.path.get_ident()?.to_string();
            if ctx.instr_args.iter().any(|a| *a == id) {
                Some(Operand::InstrArg(id))
            } else {
                None
            }
        }
        // `a.key()`, `a.lamports()`, `a.data_len()`
        Expr::MethodCall(mc) if mc.args.is_empty() => {
            let base = match mc.receiver.as_ref() {
                Expr::Path(p) => p.path.get_ident()?.to_string(),
                _ => return None,
            };
            let i = *ctx.index_of.get(&base)?;
            match mc.method.to_string().as_str() {
                "key" => Some(Operand::Key(i)),
                "lamports" => Some(Operand::Lamports(i)),
                "data_len" => Some(Operand::DataLen(i)),
                _ => None,
            }
        }
        // `a.owner`, `a.is_signer`, `a.is_writable`, `a.executable`, or a data-field path
        Expr::Field(fe) => {
            let mut path = Vec::new();
            let mut cur = fe;
            let base = loop {
                let seg = match &cur.member {
                    syn::Member::Named(id) => id.to_string(),
                    syn::Member::Unnamed(_) => return None,
                };
                path.push(seg);
                match cur.base.as_ref() {
                    Expr::Field(inner) => cur = inner,
                    Expr::Path(p) => break p.path.get_ident()?.to_string(),
                    _ => return None,
                }
            };
            path.reverse();
            let i = *ctx.index_of.get(&base)?;
            // RESOLUTION ORDER, and it is load-bearing: a TYPED `Account<'info, T>` is read as
            // its DATA first, because that is what stock Anchor means. `Account<T>: Deref<T>`
            // exposes no `owner`/`is_signer` of its own, so `vault.owner` on a typed account is
            // `T::owner` under real Anchor — resolving it to the AccountInfo owner instead would
            // silently check a different thing than the developer wrote. Untyped wrappers have
            // no data to read, so for them the metadata names are the only meaning available.
            if ctx.inner_ty.contains_key(&base) {
                // SINGLE-SEGMENT ONLY, on purpose. Lean's `locateField'` walks a full path, but
                // `ty_map::map_ty` cannot produce a nested `Ty::Struct` today, so a descriptor
                // NEVER contains one — a two-segment path would compile into the sublanguage and
                // then reject every account forever. Falling out instead hands `vault.inner.x`
                // to the escape hatch, which runs the developer's real Rust. Widen this the day
                // `map_ty` learns nested structs, not before.
                if path.len() != 1 {
                    return None;
                }
                return Some(Operand::Field(i, path));
            }
            if path.len() == 1 {
                match path[0].as_str() {
                    "owner" => return Some(Operand::Owner(i)),
                    "is_signer" => return Some(Operand::IsSigner(i)),
                    "is_writable" => return Some(Operand::IsWritable(i)),
                    "executable" => return Some(Operand::Executable(i)),
                    _ => {}
                }
            }
            // A data field on an untyped wrapper: no modelled layout, so no operand. Falls out
            // of the sublanguage rather than fabricating an offset.
            None
        }
        Expr::Paren(p) => operand(&p.expr, ctx),
        // `Group` is what a macro-substituted expression looks like after token capture; it is
        // invisible in source, so it must be transparent here too.
        Expr::Group(g) => operand(&g.expr, ctx),
        Expr::Reference(r) => operand(&r.expr, ctx),
        Expr::Unary(syn::ExprUnary { op: UnOp::Deref(_), expr, .. }) => operand(expr, ctx),
        _ => None,
    }
}

/// `None` => outside the sublanguage => escape hatch. Never an error.
pub(crate) fn compile_expr(e: &Expr, ctx: &ExprCtx) -> Option<VExpr> {
    match e {
        Expr::Paren(p) => compile_expr(&p.expr, ctx),
        Expr::Group(g) => compile_expr(&g.expr, ctx),
        Expr::Binary(b) => {
            let cmp = match b.op {
                BinOp::Eq(_) => Some(Cmp::Eq),
                BinOp::Ne(_) => Some(Cmp::Ne),
                BinOp::Lt(_) => Some(Cmp::Lt),
                BinOp::Le(_) => Some(Cmp::Le),
                BinOp::Gt(_) => Some(Cmp::Gt),
                BinOp::Ge(_) => Some(Cmp::Ge),
                _ => None,
            };
            if let Some(c) = cmp {
                return Some(VExpr::Cmp(c, operand(&b.left, ctx)?, operand(&b.right, ctx)?));
            }
            match b.op {
                BinOp::And(_) => Some(VExpr::And(
                    Box::new(compile_expr(&b.left, ctx)?),
                    Box::new(compile_expr(&b.right, ctx)?),
                )),
                BinOp::Or(_) => Some(VExpr::Or(
                    Box::new(compile_expr(&b.left, ctx)?),
                    Box::new(compile_expr(&b.right, ctx)?),
                )),
                _ => None,
            }
        }
        Expr::Unary(syn::ExprUnary { op: UnOp::Not(_), expr, .. }) => {
            Some(VExpr::Not(Box::new(compile_expr(expr, ctx)?)))
        }
        other => Some(VExpr::Truthy(operand(other, ctx)?)),
    }
}

// ── Lean emission ─────────────────────────────────────────────────────────────────────────

impl Operand {
    /// The Lean `Operand` term, UNPARENTHESISED. Callers wrap it — every use site in the Lean
    /// `Expr` constructors is an argument position, so parens are always needed there.
    fn to_lean(&self) -> String {
        match self {
            Operand::LitNat(n) => format!("Operand.lit (Value.nat {n})"),
            // Lean needs the parens around a negative numeral.
            Operand::LitInt(i) => format!("Operand.lit (Value.int ({i}))"),
            Operand::LitBool(b) => format!("Operand.lit (Value.bool {b})"),
            Operand::Field(i, path) => {
                let segs: Vec<String> = path.iter().map(|s| format!("\"{s}\"")).collect();
                format!("Operand.field {i} [{}]", segs.join(", "))
            }
            Operand::Key(i) => format!("Operand.key {i}"),
            Operand::Owner(i) => format!("Operand.owner {i}"),
            Operand::Lamports(i) => format!("Operand.lamports {i}"),
            Operand::DataLen(i) => format!("Operand.dataLen {i}"),
            Operand::IsSigner(i) => format!("Operand.isSigner {i}"),
            Operand::IsWritable(i) => format!("Operand.isWritable {i}"),
            Operand::Executable(i) => format!("Operand.executable {i}"),
            Operand::InstrArg(n) => format!("Operand.instrArg \"{n}\""),
        }
    }

    fn is_instr_arg(&self) -> bool {
        matches!(self, Operand::InstrArg(_))
    }
}

impl VExpr {
    /// The Lean `Expr` term, UNPARENTHESISED at the top level. `lib.rs` wraps the whole thing
    /// as `Constraint.expr (<this>)`; nested occurrences are parenthesised here.
    pub(crate) fn to_lean(&self) -> String {
        match self {
            VExpr::Cmp(op, l, r) => format!(
                "Expr.cmp {} ({}) ({})",
                op.lean(),
                l.to_lean(),
                r.to_lean()
            ),
            VExpr::And(l, r) => format!("Expr.and ({}) ({})", l.to_lean(), r.to_lean()),
            VExpr::Or(l, r) => format!("Expr.or ({}) ({})", l.to_lean(), r.to_lean()),
            VExpr::Not(e) => format!("Expr.not ({})", e.to_lean()),
            VExpr::Truthy(o) => format!("Expr.truthy ({})", o.to_lean()),
        }
    }

    /// Does any operand read a named `#[instruction(...)]` argument? Gates emission of the
    /// `INSTR_ARGS` const that `Operand::InstrArg`'s codegen references (see `lib.rs`).
    pub(crate) fn uses_instr_arg(&self) -> bool {
        match self {
            VExpr::Cmp(_, l, r) => l.is_instr_arg() || r.is_instr_arg(),
            VExpr::And(l, r) | VExpr::Or(l, r) => l.uses_instr_arg() || r.uses_instr_arg(),
            VExpr::Not(e) => e.uses_instr_arg(),
            VExpr::Truthy(o) => o.is_instr_arg(),
        }
    }
}

// ── Rust emission ─────────────────────────────────────────────────────────────────────────

/// A `Value` read out of borrowed bytes is reconstructed constructor-by-constructor so the
/// result carries NO borrow of the `Ref` guard it came from. `Value::Bytes` is the only
/// borrowing variant and `read_val` never yields it (every aggregate arm fails closed), so
/// mapping it to `None` here is unreachable-but-closed rather than a behavioural choice.
///
/// The `'static` annotation on the caller's binding is what makes this load-bearing instead of
/// decorative: it turns any future leak of a borrow into a compile error in THIS crate.
///
/// MUST be applied INSIDE the scope that holds the `Ref` guard — a `Value` that escapes the
/// guard's block first is already a borrow error, and no amount of rebinding afterwards helps.
fn rebind_value(inner: TokenStream2) -> TokenStream2 {
    quote! {
        match #inner {
            ::core::option::Option::Some(::verified_anchor::layout::Value::Nat(__n)) =>
                ::core::option::Option::Some(::verified_anchor::layout::Value::Nat(__n)),
            ::core::option::Option::Some(::verified_anchor::layout::Value::Int(__i)) =>
                ::core::option::Option::Some(::verified_anchor::layout::Value::Int(__i)),
            ::core::option::Option::Some(::verified_anchor::layout::Value::Bool(__b)) =>
                ::core::option::Option::Some(::verified_anchor::layout::Value::Bool(__b)),
            ::core::option::Option::Some(::verified_anchor::layout::Value::Key(__k)) =>
                ::core::option::Option::Some(::verified_anchor::layout::Value::Key(__k)),
            ::core::option::Option::Some(::verified_anchor::layout::Value::Bytes(_))
            | ::core::option::Option::None => ::core::option::Option::None,
        }
    }
}

/// Bind an operand body to a `'static` `Option<Value>` so no borrow can escape it.
fn as_static_value(body: TokenStream2) -> TokenStream2 {
    quote! {
        {
            let __v: ::core::option::Option<::verified_anchor::layout::Value<'static>> = #body;
            __v
        }
    }
}

impl Operand {
    /// An `Option<Value<'static>>` expression. Mirrors Lean `evalOperand` arm for arm: `None`
    /// means "could not evaluate", which every caller reads as "not satisfied".
    fn to_tokens(&self, ctx: &ExprCtx) -> TokenStream2 {
        let some = |v: TokenStream2| as_static_value(quote! { ::core::option::Option::Some(#v) });
        match self {
            Operand::LitNat(n) => {
                let n = *n;
                some(quote! { ::verified_anchor::layout::Value::Nat(#n) })
            }
            Operand::LitInt(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Int(#i) })
            }
            Operand::LitBool(b) => {
                let b = *b;
                some(quote! { ::verified_anchor::layout::Value::Bool(#b) })
            }
            // Lean: `locateField' path a.data` then `readVal`, both starting from offset 8 —
            // past the Anchor discriminator. Only a typed `Account<'info, T>` has a layout, and
            // `operand()` above only builds `Field` for those, so `inner_ty` always resolves.
            Operand::Field(i, path) => {
                let i = *i;
                let inner = ctx
                    .inner_ty
                    .iter()
                    .find(|(name, _)| ctx.index_of.get(*name) == Some(&i))
                    .map(|(_, t)| t.clone())
                    .expect("Operand::Field on a field with no typed layout");
                let segs: Vec<&str> = path.iter().map(|s| s.as_str()).collect();
                let seg0 = segs[0];
                // The rebind sits INSIDE the `Ok(__data)` arm, while the `Ref` guard is alive.
                let read = rebind_value(quote! {
                    ::verified_anchor::layout::locate(&__ty, &[#(#segs),*], &__data, 8)
                        .and_then(|(__off, __fty)|
                            ::verified_anchor::layout::read_val(&__fty, &__data, __off))
                });
                as_static_value(quote! {
                    {
                        // BUILD-TIME guard, the same one `has_one` carries and for the same
                        // reason: when the target is absent from the descriptor, `locate`
                        // returns `None` at runtime and the constraint rejects EVERY account,
                        // including legitimate ones, silently. `#[derive(AccountData)]`
                        // truncates the layout at the first field whose type `map_ty` cannot
                        // map, so correct-looking Anchor code can land here. Deciding it from
                        // the descriptor alone turns a bricked instruction into a build error.
                        const _: () = ::core::assert!(
                            ::verified_anchor::layout::has_top_level_field(
                                <#inner as ::verified_anchor::AccountData>::LAYOUT, #seg0),
                            ::core::concat!(
                                "verified-anchor: `constraint = ...` reads `", #seg0,
                                "`, which cannot be located in the Borsh layout of `",
                                ::core::stringify!(#inner), "`. Either `", #seg0,
                                "` is not a field of `", ::core::stringify!(#inner),
                                "`, or an EARLIER field of `", ::core::stringify!(#inner),
                                "` has a type verified-anchor cannot map yet (fixed-size arrays, \
                                 nested structs, enums): the layout is truncated at the first \
                                 such field, because every offset behind it is unknowable. Move \
                                 the field ahead of that one, or give that field a mappable type."),
                        );
                        match accounts[#i].try_borrow_data() {
                            ::core::result::Result::Ok(__data) => {
                                let __ty = <#inner as ::verified_anchor::AccountData>::LAYOUT;
                                #read
                            }
                            // A borrow conflict is not evidence the constraint holds.
                            ::core::result::Result::Err(_) => ::core::option::Option::None,
                        }
                    }
                })
            }
            Operand::Key(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Key(*accounts[#i].key) })
            }
            Operand::Owner(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Key(*accounts[#i].owner) })
            }
            Operand::Lamports(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Nat(accounts[#i].lamports() as u128) })
            }
            Operand::DataLen(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Nat(accounts[#i].data_len() as u128) })
            }
            Operand::IsSigner(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Bool(accounts[#i].is_signer) })
            }
            Operand::IsWritable(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Bool(accounts[#i].is_writable) })
            }
            Operand::Executable(i) => {
                let i = *i;
                some(quote! { ::verified_anchor::layout::Value::Bool(accounts[#i].executable) })
            }
            // Lean: `locate (Ty.struct s.instrArgs) [n] c.instrData 0` then `readVal`. `INSTR_ARGS`
            // is the same const the `name.as_bytes()` seeds use; `lib.rs` widens its emission gate
            // so it is in scope here even when no seed needs it.
            Operand::InstrArg(n) => {
                let n = n.as_str();
                let read = rebind_value(quote! {
                    ::verified_anchor::layout::locate(&__ty, &[#n], instr_data, 0)
                        .and_then(|(__off, __fty)|
                            ::verified_anchor::layout::read_val(&__fty, instr_data, __off))
                });
                as_static_value(quote! {
                    {
                        let __ty = ::verified_anchor::layout::Ty::Struct(INSTR_ARGS);
                        #read
                    }
                })
            }
        }
    }
}

impl Cmp {
    /// The `evalCmp` arms for THIS operator, over two already-evaluated `Value`s.
    ///
    /// NUMERIC pairs — `Nat`/`Nat`, `Int`/`Int`, AND the mixed pairings — compare as
    /// mathematical integers, mirroring Lean's `Value.toInt?` + `evalCmp`. The mixed case is
    /// load-bearing, not a nicety: under the old constructor-sensitive comparison,
    /// `constraint = delta != 0` on an `i64` field was a TAUTOLOGY (`Int(-1) != Nat(0)` is
    /// `true` for EVERY `delta`, zero included), i.e. a guard the developer wrote and the
    /// codegen silently disabled. `delta == 0` was the mirror-image brick.
    ///
    /// The earlier "do NOT add a coercion, `-1i64` and `u64::MAX` share their bytes" reasoning
    /// confused decode time with comparison time: `read_val` has already used the declared `Ty`
    /// to pick the sign, so by here there are two distinct, unambiguous integers and comparing
    /// them numerically picks nothing. Do not re-narrow this on the old argument.
    ///
    /// NON-numeric pairs are unchanged: `eq`/`ne` stay TOTAL over `Value` (Rust's derived
    /// `PartialEq` matches Lean's derived `DecidableEq` for those), the four orderings yield
    /// `None`, so `key < nat` REJECTS rather than silently passing.
    fn apply(&self, l: TokenStream2, r: TokenStream2) -> TokenStream2 {
        // The one place Rust needs a guard Lean does not: `Int` is unbounded in Lean, but
        // `u128` and `i128` do not share a range, so a `Nat` above `i128::MAX` cannot be cast.
        // It is also unambiguously larger than any `i128`, hence `Greater`/`Less` directly.
        // Specified by the boundary examples in `Codegen/ExampleGenerated.lean`.
        let ord = quote! {
            match (__l, __r) {
                (
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Nat(__a)),
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Nat(__b)),
                ) => ::core::option::Option::Some(::core::cmp::Ord::cmp(&__a, &__b)),
                (
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Int(__a)),
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Int(__b)),
                ) => ::core::option::Option::Some(::core::cmp::Ord::cmp(&__a, &__b)),
                (
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Nat(__a)),
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Int(__b)),
                ) => ::core::option::Option::Some(
                    if __a > i128::MAX as u128 {
                        ::core::cmp::Ordering::Greater
                    } else {
                        ::core::cmp::Ord::cmp(&(__a as i128), &__b)
                    }
                ),
                (
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Int(__a)),
                    ::core::option::Option::Some(::verified_anchor::layout::Value::Nat(__b)),
                ) => ::core::option::Option::Some(
                    if __b > i128::MAX as u128 {
                        ::core::cmp::Ordering::Less
                    } else {
                        ::core::cmp::Ord::cmp(&__a, &(__b as i128))
                    }
                ),
                // Non-numeric on at least one side: Lean's `Value.toInt?` is `none` here too.
                _ => ::core::option::Option::None,
            }
        };
        // Which `Ordering`s satisfy this operator.
        let sat = match self {
            Cmp::Eq => quote! { __o == ::core::cmp::Ordering::Equal },
            Cmp::Ne => quote! { __o != ::core::cmp::Ordering::Equal },
            Cmp::Lt => quote! { __o == ::core::cmp::Ordering::Less },
            Cmp::Le => quote! { __o != ::core::cmp::Ordering::Greater },
            Cmp::Gt => quote! { __o == ::core::cmp::Ordering::Greater },
            Cmp::Ge => quote! { __o != ::core::cmp::Ordering::Less },
        };
        // Lean's `| _, _ =>` fallback arm, reached only when a side is non-numeric.
        let fallback = match self {
            Cmp::Eq | Cmp::Ne => {
                let op = if matches!(self, Cmp::Eq) { quote! { == } } else { quote! { != } };
                quote! {
                    match (__l, __r) {
                        (::core::option::Option::Some(__a), ::core::option::Option::Some(__b)) =>
                            ::core::option::Option::Some(__a #op __b),
                        _ => ::core::option::Option::None,
                    }
                }
            }
            _ => quote! { ::core::option::Option::None },
        };
        quote! {
            {
                // `Option<Value<'static>>` is `Copy`, so both bindings are readable twice: once
                // for the numeric ordering, once for the non-numeric `eq`/`ne` fallback.
                let __l: ::core::option::Option<::verified_anchor::layout::Value<'static>> = #l;
                let __r: ::core::option::Option<::verified_anchor::layout::Value<'static>> = #r;
                let __ord: ::core::option::Option<::core::cmp::Ordering> = #ord;
                match __ord {
                    ::core::option::Option::Some(__o) => ::core::option::Option::Some(#sat),
                    ::core::option::Option::None => #fallback,
                }
            }
        }
    }
}

impl VExpr {
    /// An `Option<bool>` expression mirroring Lean `evalExpr`.
    fn to_tokens(&self, ctx: &ExprCtx) -> TokenStream2 {
        match self {
            VExpr::Cmp(op, l, r) => op.apply(l.to_tokens(ctx), r.to_tokens(ctx)),
            // ── STRICT `and`/`or`. READ BEFORE CHANGING. ──────────────────────────────────
            //
            // Both operands are bound to `let` STATEMENTS, which run unconditionally, and only
            // then combined. This is not a style preference — it is the codegen half of the
            // milestone's headline guarantee, "verified-anchor never accepts an account set the
            // contract rejects".
            //
            // Lean's `evalExpr` binds both sides through the `Option` monad
            // (`do let a ← ..; let b ← ..; pure (a || b)`), so an unevaluable operand poisons
            // the whole expression whatever the other side says. Lowering to Rust's native
            // `||` would short-circuit `true || <unevaluable>` to `true` — ACCEPT — where the
            // contract yields `none` — REJECT. That single asymmetry is the whole reason this
            // is spelled out longhand. (`and` has no such gap: `false && <unevaluable>`
            // rejects either way. It is written strictly anyway so the two arms cannot drift,
            // and so the `and` half of a mixed expression cannot skip a sub-`or`.)
            //
            // `tests/behavior.rs::strict_or_rejects_when_right_operand_is_unevaluable` fails if
            // this is ever "optimized" into a native operator.
            VExpr::And(l, r) => {
                let (l, r) = (l.to_tokens(ctx), r.to_tokens(ctx));
                quote! {
                    {
                        let __l: ::core::option::Option<bool> = #l;
                        let __r: ::core::option::Option<bool> = #r;
                        match (__l, __r) {
                            (::core::option::Option::Some(__a), ::core::option::Option::Some(__b)) =>
                                ::core::option::Option::Some(__a && __b),
                            _ => ::core::option::Option::None,
                        }
                    }
                }
            }
            VExpr::Or(l, r) => {
                let (l, r) = (l.to_tokens(ctx), r.to_tokens(ctx));
                quote! {
                    {
                        let __l: ::core::option::Option<bool> = #l;
                        let __r: ::core::option::Option<bool> = #r;
                        match (__l, __r) {
                            (::core::option::Option::Some(__a), ::core::option::Option::Some(__b)) =>
                                ::core::option::Option::Some(__a || __b),
                            _ => ::core::option::Option::None,
                        }
                    }
                }
            }
            VExpr::Not(e) => {
                let e = e.to_tokens(ctx);
                quote! {
                    match #e {
                        ::core::option::Option::Some(__a) => ::core::option::Option::Some(!__a),
                        ::core::option::Option::None => ::core::option::Option::None,
                    }
                }
            }
            // Lean: only a `.bool` value is truthy; anything else is unevaluable, NOT false.
            VExpr::Truthy(o) => {
                let o = o.to_tokens(ctx);
                quote! {
                    match #o {
                        ::core::option::Option::Some(::verified_anchor::layout::Value::Bool(__b)) =>
                            ::core::option::Option::Some(__b),
                        _ => ::core::option::Option::None,
                    }
                }
            }
        }
    }

    /// The full runtime check for one `constraint = <expr>` on `field`.
    ///
    /// FAILS CLOSED on `None`: the comparison is `!= Some(true)`, not `== Some(false)`, so an
    /// unevaluable expression rejects exactly as Lean's `genConstraint` does
    /// (`(evalExpr s c e).allB (fun b => b)`, which is `false` on `none`). `expr_src` is the
    /// developer's source text, carried into the error so a rejection names the constraint.
    pub(crate) fn to_tokens_check(
        &self,
        field: &str,
        expr_src: &str,
        ctx: &ExprCtx,
    ) -> TokenStream2 {
        let body = self.to_tokens(ctx);
        quote! {
            {
                let __ok: ::core::option::Option<bool> = #body;
                if __ok != ::core::option::Option::Some(true) {
                    return Err(::verified_anchor::VAError::ConstraintViolated {
                        field: #field,
                        expr: #expr_src,
                    });
                }
            }
        }
    }
}
