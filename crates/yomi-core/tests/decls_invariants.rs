//! Tests of invariants of declaration files.
//!
//! Using a hand-written seeded random generator, makes declaration files with random types, commands, variables, aliases
//! and sugars, and checks the following.
//! - They can be read (`from_decls`).
//! - `export_decls` → `from_decls` → `export_decls` gives the same string.
//! - Broken declarations (bytes replaced or truncated) do not panic; the result is `Ok` or `Err`.
//!
//! Environment variables: `YOMI_INVARIANT_CASES` (number of cases; default 200), `YOMI_INVARIANT_SEED` (seed).

mod common;
use common::rng::{self, Rng};

use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

use yomi_core::Registry;

fn cases() -> usize {
    rng::case_count_or(200)
}

fn seed() -> u64 {
    rng::base_seed_or(20_261_118)
}

/// A model of types.
#[derive(Clone)]
enum T {
    Int,
    Float,
    Bool,
    Str,
    Identity,
    List(Box<T>),
    Opt(Box<T>),
    /// Keys are `str`.
    Map(Box<T>),
    /// The number of a type (`struct` or `enum`).
    User(usize),
}

struct Decl {
    is_struct: bool,
    fields: Vec<T>,
    /// The arguments of an `enum` variant.
    variants: Vec<Vec<T>>,
}

fn name(t: &T) -> String {
    match t {
        T::Int => "int".into(),
        T::Float => "float".into(),
        T::Bool => "bool".into(),
        T::Str => "str".into(),
        T::Identity => "Identity".into(),
        T::List(i) => format!("list<{}>", name(i)),
        T::Opt(i) => format!("Option<{}>", name(i)),
        T::Map(i) => format!("map<str, {}>", name(i)),
        T::User(k) => format!("t.T{k}"),
    }
}

fn gen_ty(r: &mut Rng, avail: usize, depth: usize) -> T {
    let top = if depth >= 3 { 6 } else { 9 };
    match r.below(top) {
        0 => T::Int,
        1 => T::Float,
        2 => T::Bool,
        3 => T::Str,
        4 => T::Identity,
        5 if avail > 0 => T::User(r.below(avail)),
        5 => T::Int,
        6 => T::List(Box::new(gen_ty(r, avail, depth + 1))),
        7 => T::Opt(Box::new(gen_ty(r, avail, depth + 1))),
        _ => T::Map(Box::new(gen_ty(r, avail, depth + 1))),
    }
}

fn gen_str(r: &mut Rng) -> String {
    const POOL: &[&str] = &["a", "あ", "\\\"", "\\n", "\\\\", "😀", "x y", "\\u0001"];
    (0..r.below(4)).map(|_| POOL[r.below(POOL.len())]).collect()
}

/// A JSON value that fits the type.
fn gen_value(r: &mut Rng, t: &T, types: &[Decl], out: &mut String) {
    match t {
        T::Int => write!(out, "{}", r.next() as i64 >> r.below(60)).unwrap(),
        T::Float => {
            let x = (r.next() % 20_000) as f64 / 8.0 - 1250.0;
            // Use both notations `3` and `3.0`
            if r.chance(30, 100) && x.fract() == 0.0 {
                write!(out, "{}", x as i64).unwrap()
            } else {
                write!(out, "{x}").unwrap()
            }
        }
        T::Bool => out.push_str(if r.chance(50, 100) { "true" } else { "false" }),
        T::Str => write!(out, "\"{}\"", gen_str(r)).unwrap(),
        T::Identity => write!(
            out,
            "{{\"id\":\"{}\",\"name\":\"{}\"}}",
            gen_str(r),
            gen_str(r)
        )
        .unwrap(),
        T::List(i) => {
            out.push('[');
            for k in 0..r.below(3) {
                if k > 0 {
                    out.push(',');
                }
                gen_value(r, i, types, out);
            }
            out.push(']');
        }
        T::Opt(i) => {
            if r.chance(50, 100) {
                out.push_str("null");
            } else {
                out.push_str("{\"some\":");
                gen_value(r, i, types, out);
                out.push('}');
            }
        }
        T::Map(i) => {
            out.push('[');
            for k in 0..r.below(3) {
                if k > 0 {
                    out.push(',');
                }
                write!(out, "[\"k{k}\",").unwrap();
                gen_value(r, i, types, out);
                out.push(']');
            }
            out.push(']');
        }
        T::User(k) => {
            let d = &types[*k];
            if d.is_struct {
                out.push('{');
                for (n, f) in d.fields.iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    write!(out, "\"f{n}\":").unwrap();
                    gen_value(r, f, types, out);
                }
                out.push('}');
            } else {
                let v = r.below(d.variants.len());
                write!(out, "{{\"variant\":\"V{v}\",\"args\":[").unwrap();
                for (n, a) in d.variants[v].iter().enumerate() {
                    if n > 0 {
                        out.push(',');
                    }
                    gen_value(r, a, types, out);
                }
                out.push_str("]}");
            }
        }
    }
}

/// A random declaration file (a valid one).
fn gen_decls(r: &mut Rng) -> String {
    let mut types: Vec<Decl> = Vec::new();
    let n_types = r.below(6);
    for k in 0..n_types {
        let is_struct = r.chance(50, 100);
        let mut d = Decl {
            is_struct,
            fields: Vec::new(),
            variants: Vec::new(),
        };
        if is_struct {
            d.fields = (0..r.below(4)).map(|_| gen_ty(r, k, 0)).collect();
        } else {
            d.variants = (0..=r.below(3))
                .map(|_| (0..r.below(3)).map(|_| gen_ty(r, k, 0)).collect())
                .collect();
        }
        types.push(d);
    }
    let mut out = String::from("{\"format\":\"yomi-decls\",\"version\":1,\"packages\":[");
    let n_pkg = 1 + r.below(3);
    for p in 0..n_pkg {
        if p > 0 {
            out.push(',');
        }
        write!(out, "{{\"name\":\"p{p}\",\"version\":{}}}", 1 + r.below(3)).unwrap();
    }
    // Write the types in shuffled order (the reader registers used types first)
    out.push_str("],\"types\":[");
    let mut order: Vec<usize> = (0..types.len()).collect();
    for i in (1..order.len()).rev() {
        order.swap(i, r.below(i + 1));
    }
    for (n, &k) in order.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        let d = &types[k];
        if d.is_struct {
            write!(
                out,
                "{{\"name\":\"t.T{k}\",\"kind\":\"struct\",\"fields\":["
            )
            .unwrap();
            for (i, f) in d.fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write!(out, "{{\"name\":\"f{i}\",\"type\":\"{}\"}}", name(f)).unwrap();
            }
        } else {
            write!(
                out,
                "{{\"name\":\"t.T{k}\",\"kind\":\"enum\",\"variants\":["
            )
            .unwrap();
            for (v, args) in d.variants.iter().enumerate() {
                if v > 0 {
                    out.push(',');
                }
                write!(out, "{{\"name\":\"V{v}\",\"fields\":[").unwrap();
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write!(out, "{{\"type\":\"{}\"}}", name(a)).unwrap();
                }
                out.push_str("]}");
            }
        }
        if r.chance(30, 100) {
            write!(out, "],\"doc\":\"型 {k}\"}}").unwrap();
        } else {
            out.push_str("]}");
        }
    }
    out.push_str("],\"commands\":[");
    let n_cmd = r.below(8);
    let mut stops: Vec<(String, bool)> = Vec::new();
    for c in 0..n_cmd {
        if c > 0 {
            out.push(',');
        }
        let kind = ["stop", "effect", "query"][r.below(3)];
        let pkg = format!("p{}", r.below(n_pkg));
        let cname = if kind == "stop" && r.chance(50, 100) {
            format!("h.c{c}")
        } else {
            format!("{pkg}.c{c}")
        };
        write!(out, "{{\"name\":\"{cname}\",\"kind\":\"{kind}\"").unwrap();
        if kind != "stop" || cname.starts_with('p') {
            write!(out, ",\"package\":\"{}\"", cname.split('.').next().unwrap()).unwrap();
        }
        out.push_str(",\"params\":[");
        let n_params = r.below(4);
        for i in 0..n_params {
            if i > 0 {
                out.push(',');
            }
            let t = gen_ty(r, types.len(), 0);
            if r.chance(50, 100) {
                write!(out, "{{\"name\":\"a{i}\",\"type\":\"{}\"}}", name(&t)).unwrap();
            } else {
                write!(out, "{{\"type\":\"{}\"}}", name(&t)).unwrap();
            }
        }
        out.push(']');
        let ret = if kind == "query" || r.chance(50, 100) {
            let t = gen_ty(r, types.len(), 0);
            format!("\"{}\"", name(&t))
        } else {
            "null".to_string()
        };
        write!(out, ",\"returns\":{ret}").unwrap();
        if r.chance(40, 100) {
            write!(out, ",\"aliases\":[\"old{}.c{c}\"]", r.below(2)).unwrap();
        }
        if r.chance(30, 100) {
            write!(out, ",\"doc\":\"{}\"", gen_str(r)).unwrap();
        }
        out.push('}');
        if kind == "stop" {
            stops.push((cname, n_params == 1));
        }
    }
    out.push_str("],\"variables\":[");
    for v in 0..r.below(5) {
        if v > 0 {
            out.push(',');
        }
        let t = gen_ty(r, types.len(), 0);
        let kind = if r.chance(50, 100) {
            "save"
        } else {
            "persistent"
        };
        write!(
            out,
            "{{\"name\":\"v.x{v}\",\"type\":\"{}\",\"kind\":\"{kind}\",\"default\":",
            name(&t)
        )
        .unwrap();
        gen_value(r, &t, &types, &mut out);
        write!(out, ",\"read_only\":{}}}", r.chance(30, 100)).unwrap();
    }
    out.push_str("],\"sugar\":{");
    if r.chance(50, 100) {
        out.push_str("\"quote\":null");
    }
    out.push('}');
    out.push('}');
    out
}

#[test]
fn random_declarations_round_trip_to_same_string() {
    let mut rng = Rng::new(seed());
    for case in 0..cases() {
        let src = gen_decls(&mut rng);
        let r1 = Registry::from_decls(&src)
            .unwrap_or_else(|e| panic!("ケース {case}: 読めない: {e}\n{src}"));
        let e1 = r1.export_decls();
        let r2 = Registry::from_decls(&e1)
            .unwrap_or_else(|e| panic!("ケース {case}: 書き出しを読めない: {e}\n{e1}"));
        let e2 = r2.export_decls();
        assert_eq!(e1, e2, "ケース {case}\n{src}");
        // The same registry gives the same string
        assert_eq!(e1, r1.export_decls());
    }
}

#[test]
fn broken_declarations_finish_loading_without_panic() {
    let mut rng = Rng::new(seed() ^ 0xABCD);
    for case in 0..cases() {
        let src = gen_decls(&mut rng);
        let json = Registry::from_decls(&src).unwrap().export_decls();
        let mut bytes = json.into_bytes();
        for _ in 0..1 + rng.below(4) {
            match rng.below(3) {
                0 if !bytes.is_empty() => {
                    let i = rng.below(bytes.len());
                    bytes[i] = b"{}[],:\"\\0aZ \n"[rng.below(13)];
                }
                1 if !bytes.is_empty() => {
                    let i = rng.below(bytes.len());
                    bytes.truncate(i);
                }
                _ if !bytes.is_empty() => {
                    let i = rng.below(bytes.len());
                    bytes.remove(i);
                }
                _ => {}
            }
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let result = catch_unwind(AssertUnwindSafe(|| Registry::from_decls(&text)));
        match result {
            Ok(Ok(reg)) => {
                // If it could be read, it can be written and read again
                let e = reg.export_decls();
                let again =
                    Registry::from_decls(&e).unwrap_or_else(|x| panic!("ケース {case}: {x}\n{e}"));
                assert_eq!(again.export_decls(), e, "ケース {case}");
            }
            Ok(Err(_)) => {}
            Err(_) => panic!("ケース {case}: パニック\n{text}"),
        }
    }
}
