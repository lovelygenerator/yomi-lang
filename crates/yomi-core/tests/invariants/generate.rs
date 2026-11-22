//! Generating valid scripts

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    Int,
    Float,
    Bool,
    Str,
}

/// The plan of a module to generate (decided first so other modules can refer to it).
#[derive(Clone)]
pub(super) struct ModPlan {
    pub(super) name: String,
    /// Modules to `import` (indices).
    pub(super) imports: Vec<usize>,
    /// Its own variables (excluding `fuel`).
    /// Name, type, whether it is a `persistent var` (false means `save var`).
    pub(super) globals: Vec<(String, Kind, bool)>,
    pub(super) labels: Vec<LabelPlan>,
}

/// The plan of labels. Labels without parameters and return values can be targets of `jump`.
#[derive(Clone)]
pub(super) struct LabelPlan {
    pub(super) name: String,
    pub(super) params: Vec<Kind>,
    pub(super) ret: Option<Kind>,
}

impl LabelPlan {
    pub(super) fn plain(&self) -> bool {
        self.params.is_empty() && self.ret.is_none()
    }
}

pub(super) struct Gen<'r> {
    pub(super) rng: &'r mut Rng,
    pub(super) out: String,
    /// Visible variables (module variables first; pushed per block).
    pub(super) scopes: Vec<Vec<(String, Kind)>>,
    pub(super) next_local: usize,
    pub(super) next_id: usize,
    pub(super) plans: Vec<ModPlan>,
    /// The module being written.
    pub(super) cur: usize,
    /// The return type of the label being written (if it has one, `jump` is not written).
    pub(super) ret: Option<Kind>,
    /// Whether the label being written has parameters or a return type (`return` can be written).
    pub(super) callable: bool,
    /// The nesting depth of `while` being read (whether `break` / `continue` can be placed).
    pub(super) loop_depth: usize,
    /// The name of the label being written (used for positions in `seen`).
    pub(super) cur_label: String,
}

pub(super) fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Int => "int",
        Kind::Float => "float",
        Kind::Bool => "bool",
        Kind::Str => "str",
    }
}

pub(super) const INT_GLOBALS: &[&str] = &["gold", "hp", "金", "n"];
pub(super) const FLOAT_GLOBALS: &[&str] = &["ratio", "速度"];
pub(super) const BOOL_GLOBALS: &[&str] = &["flag", "済"];
pub(super) const STR_GLOBALS: &[&str] = &["name", "名前"];
pub(super) const STR_LITERALS: &[&str] = &[
    "\"\"",
    "\"あ\"",
    "\"ミナ\"",
    "\"a b\"",
    "\"q\\\"q\"",
    "\"\\\\\"",
    "\"x\\ty\"",
];
pub(super) const TEXT_START: &[&str] = &[
    "おはよう",
    "今日は",
    "さて、",
    "そして",
    "ええと",
    "\\$記号",
];
pub(super) const TEXT_CHUNKS: &[&str] = &[
    "あ",
    "いう",
    "テスト",
    "、",
    "。",
    " ",
    "a b",
    "//",
    "\\=",
    "\\#",
    "\\$",
    "\\\\",
    "\\「",
    "\\」",
    "「",
    "」",
    "(",
    ")",
    "!",
    "?",
    "…",
    "𠮷",
];

impl<'r> Gen<'r> {
    pub(super) fn vars_of(&self, kind: Kind) -> Vec<String> {
        self.scopes
            .iter()
            .flatten()
            .filter(|(_, k)| *k == kind)
            .map(|(n, _)| n.clone())
            .collect()
    }

    pub(super) fn int_expr(&mut self, depth: usize) -> String {
        let vars = self.vars_of(Kind::Int);
        let choice = if depth == 0 {
            self.rng.below(2)
        } else {
            self.rng.below(11)
        };
        match choice {
            // Random numbers. Written so that the minimum is at most the maximum (the maximum is 3 or more)
            8 | 9 => format!(
                "random({}, {})",
                self.rng.range(-5, 1),
                self.nonzero(depth - 1)
            ),
            0 => self.rng.range(-9, 99).to_string(),
            1 if !vars.is_empty() => self.rng.pick(&vars).clone(),
            1 => "7".to_string(),
            2 => format!(
                "({} + {})",
                self.int_expr(depth - 1),
                self.int_expr(depth - 1)
            ),
            3 => format!(
                "({} - {})",
                self.int_expr(depth - 1),
                self.int_expr(depth - 1)
            ),
            4 => format!("({} * {})", self.int_expr(depth - 1), self.rng.range(-3, 9)),
            5 => format!(
                "({} / {})",
                self.int_expr(depth - 1),
                self.nonzero(depth - 1)
            ),
            6 => format!(
                "({} % {})",
                self.int_expr(depth - 1),
                self.nonzero(depth - 1)
            ),
            7 => format!("to_int({})", self.float_expr(depth - 1)),
            _ => format!("-{}", self.int_expr(depth - 1)),
        }
    }

    /// A floating point expression. Variables are kept within `±1000`, so up to depth 2 there is no overflow and no
    /// out-of-range `to_int`. There is no division by 0.0 either.
    pub(super) fn float_expr(&mut self, depth: usize) -> String {
        let vars = self.vars_of(Kind::Float);
        let choice = if depth == 0 {
            self.rng.below(2)
        } else {
            self.rng.below(8)
        };
        match choice {
            0 => format!("{}.{}", self.rng.range(0, 20), self.rng.range(0, 99)),
            1 if !vars.is_empty() => self.rng.pick(&vars).clone(),
            1 => "0.5".to_string(),
            2 => format!(
                "({} + {})",
                self.float_expr(depth - 1),
                self.float_expr(depth - 1)
            ),
            3 => format!(
                "({} - {})",
                self.float_expr(depth - 1),
                self.float_expr(depth - 1)
            ),
            4 => format!(
                "({} * {})",
                self.float_expr(depth - 1),
                self.float_expr(depth - 1)
            ),
            5 => {
                let d = self.float_expr(depth - 1);
                format!("({} / ({d} * {d} + 1.0))", self.float_expr(depth - 1))
            }
            6 => format!("to_float({})", self.int_expr(depth - 1)),
            _ => format!("-{}", self.float_expr(depth - 1)),
        }
    }

    /// An integer expression that is never 0 (2 to 10).
    pub(super) fn nonzero(&mut self, depth: usize) -> String {
        format!("(({} % 5 + 5) + 2)", self.int_expr(depth))
    }

    pub(super) fn bool_expr(&mut self, depth: usize) -> String {
        let vars = self.vars_of(Kind::Bool);
        let choice = if depth == 0 {
            self.rng.below(3)
        } else {
            self.rng.below(12)
        };
        match choice {
            10 if self.next_id > 0 => {
                // The stable IDs written so far in this `label`
                let k = self.rng.range(1, self.next_id as i64);
                let path = if self.rng.chance(1, 2) {
                    format!("{}#i{k}", self.cur_label)
                } else {
                    format!("{}.{}#i{k}", self.plans[self.cur].name, self.cur_label)
                };
                format!("seen(\"{path}\")")
            }
            10 | 11 => {
                let mods = self.visible_modules();
                let m = *self.rng.pick(&mods);
                let labels = self.plans[m].labels.clone();
                let l = self.rng.pick(&labels).name.clone();
                let path = self.label_path(m, &l);
                format!("visited(\"{path}\")")
            }
            0 => if self.rng.chance(1, 2) {
                "true"
            } else {
                "false"
            }
            .to_string(),
            1 | 2 if !vars.is_empty() => self.rng.pick(&vars).clone(),
            1 | 2 => "true".to_string(),
            3 => format!("!{}", self.bool_expr(depth - 1)),
            4 => {
                let op = *self.rng.pick(&["<", "<=", ">", ">=", "==", "!="]);
                format!(
                    "({} {} {})",
                    self.int_expr(depth - 1),
                    op,
                    self.int_expr(depth - 1)
                )
            }
            5 => format!(
                "({} && {})",
                self.bool_expr(depth - 1),
                self.bool_expr(depth - 1)
            ),
            6 => format!(
                "({} || {})",
                self.bool_expr(depth - 1),
                self.bool_expr(depth - 1)
            ),
            7 => {
                let op = *self.rng.pick(&["==", "!="]);
                format!(
                    "({} {} {})",
                    self.str_expr(depth - 1),
                    op,
                    self.str_expr(depth - 1)
                )
            }
            8 => {
                let op = *self.rng.pick(&["<", "<=", ">", ">=", "==", "!="]);
                format!(
                    "({} {} {})",
                    self.float_expr(depth - 1),
                    op,
                    self.float_expr(depth - 1)
                )
            }
            _ => format!(
                "({} == {})",
                self.bool_expr(depth - 1),
                self.bool_expr(depth - 1)
            ),
        }
    }

    pub(super) fn str_expr(&mut self, depth: usize) -> String {
        let vars = self.vars_of(Kind::Str);
        match self.rng.below(if depth == 0 { 2 } else { 5 }) {
            0 => self.rng.pick(STR_LITERALS).to_string(),
            1 if !vars.is_empty() => self.rng.pick(&vars).clone(),
            1 => "\"s\"".to_string(),
            4 => format!("\"比は${{{}}}です\"", self.float_expr(depth - 1)),
            2 => format!("\"値は${{{}}}です\"", self.int_expr(depth - 1)),
            _ => format!(
                "\"${{{}}}と${{{}}}\"",
                self.str_expr(depth - 1),
                self.bool_expr(depth - 1)
            ),
        }
    }

    pub(super) fn indent(&self) -> String {
        "  ".repeat(self.scopes.len())
    }

    pub(super) fn line(&mut self, s: &str) {
        let ind = self.indent();
        writeln!(self.out, "{ind}{s}").unwrap();
    }

    pub(super) fn maybe_id(&mut self) -> String {
        if self.rng.chance(2, 5) {
            self.next_id += 1;
            format!(" #i{}", self.next_id)
        } else {
            String::new()
        }
    }

    pub(super) fn plain_text(&mut self) -> String {
        let mut s = self.rng.pick(TEXT_START).to_string();
        for _ in 0..self.rng.below(5) {
            if self.rng.chance(1, 3) {
                let e = self.int_expr(2);
                write!(s, "${{{e}}}").unwrap();
            } else if self.rng.chance(1, 5) {
                let e = self.float_expr(2);
                write!(s, "${{{e}}}").unwrap();
            } else if self.rng.chance(1, 6) {
                let e = self.str_expr(1);
                write!(s, "${{{e}}}").unwrap();
            } else {
                s.push_str(self.rng.pick(TEXT_CHUNKS));
            }
        }
        // Trailing whitespace is removed, so make sure it does not mix with the separator before a stable ID
        s.trim_end().to_string()
    }

    pub(super) fn stmt(&mut self, depth: usize) {
        match self.rng.below(34) {
            29..=33 => self.bind_stmt(),
            21..=22 => self.quote_stmt(),
            23 | 24 if depth > 0 => self.choice_stmt(depth),
            0..=3 => {
                let t = self.plain_text();
                let id = self.maybe_id();
                self.line(&format!("{t}{id}"));
            }
            4 => {
                let (a, b) = (self.str_expr(2), self.int_expr(2));
                let id = self.maybe_id();
                self.line(&format!("@game.give({a}, {b}){id}"));
            }
            5 => {
                let a = self.int_expr(2);
                let id = self.maybe_id();
                self.line(&format!("@game.roll({a}){id}"));
            }
            6 => {
                let id = self.maybe_id();
                if self.rng.chance(1, 2) {
                    self.line(&format!("@game.pause(){id}"));
                } else {
                    let a = self.str_expr(2);
                    self.line(&format!("@ui.say({a}){id}"));
                }
            }
            7 => {
                self.next_local += 1;
                let name = format!("v{}", self.next_local);
                let (kind, init, ann) = match self.rng.below(4) {
                    0 => (Kind::Int, self.int_expr(2), "int"),
                    1 => (Kind::Bool, self.bool_expr(2), "bool"),
                    2 => (Kind::Float, self.float_expr(2), "float"),
                    _ => (Kind::Str, self.str_expr(2), "str"),
                };
                if self.rng.chance(1, 2) {
                    self.line(&format!("let {name}: {ann} = {init}"));
                } else {
                    self.line(&format!("let {name} = {init}"));
                }
                self.scopes.last_mut().unwrap().push((name.clone(), kind));
                if kind == Kind::Float {
                    self.clamp_float(&name);
                }
            }
            8..=9 => self.assign(),
            10..=11 if depth > 0 => self.if_stmt(depth),
            // Inside a label with a return type, do not write `jump`
            12 if self.ret.is_none() => match self.pick_jump() {
                Some(l) => self.line(&format!("jump {l}")),
                None => {
                    let t = self.plain_text();
                    self.line(&t);
                }
            },
            17 | 18 | 25 | 26 => self.call_stmt(),
            19 if self.callable => {
                // An early `return`
                let c = self.bool_expr(1);
                let value = self.ret.map(|k| self.returned(k));
                self.line(&format!("if {c} {{"));
                match value {
                    Some(v) => self.line(&format!("  return {v}")),
                    None => self.line("  return"),
                }
                self.line("}");
            }
            13 if self.rng.chance(1, 4) => self.line("end"),
            14 | 15 | 27 | 28 if depth > 0 => self.while_stmt(depth),
            16 if self.loop_depth > 0 => {
                let c = self.bool_expr(1);
                let word = if self.rng.chance(1, 2) {
                    "break"
                } else {
                    "continue"
                };
                self.line(&format!("if {c} {{"));
                self.line(&format!("  {word}"));
                self.line("}");
            }
            _ => {
                let t = self.plain_text();
                self.line(&t);
            }
        }
    }

    /// Receives the return value of a stop command (`let name = @command(...)`, `name = @command(...)`).
    /// The receiving variable is a new local variable or a visible variable of the same type.
    pub(super) fn bind_stmt(&mut self) {
        let kind = *self.rng.pick(KINDS);
        let call = match kind {
            Kind::Int => format!("@game.roll(({}) % 1000)", self.int_expr(1)),
            Kind::Str => format!("@game.ask({})", self.str_expr(1)),
            Kind::Bool => format!("@game.yes({})", self.str_expr(1)),
            Kind::Float => format!("@game.half({})", self.float_expr(1)),
        };
        let id = self.maybe_id();
        let targets: Vec<String> = self
            .vars_of(kind)
            .into_iter()
            .filter(|n| n != "fuel" && !n.ends_with(".fuel") && !n.starts_with("lp"))
            .collect();
        if !targets.is_empty() && self.rng.chance(1, 2) {
            let name = self.rng.pick(&targets).clone();
            self.line(&format!("{name} = {call}{id}"));
            if kind == Kind::Float {
                self.clamp_float(&name);
            }
            return;
        }
        self.next_local += 1;
        let name = format!("v{}", self.next_local);
        if self.rng.chance(1, 2) {
            self.line(&format!("let {name}: {} = {call}{id}", kind_name(kind)));
        } else {
            self.line(&format!("let {name} = {call}{id}"));
        }
        self.scopes.last_mut().unwrap().push((name.clone(), kind));
        if kind == Kind::Float {
            self.clamp_float(&name);
        }
    }

    /// The contents of a quote or option (may contain `${expr}`).
    pub(super) fn quote_inner(&mut self) -> String {
        let mut s = String::new();
        for _ in 0..(1 + self.rng.below(4)) {
            if self.rng.chance(1, 4) {
                let e = self.int_expr(1);
                write!(s, "${{{e}}}").unwrap();
            } else {
                s.push_str(self.rng.pick(QUOTE_CHUNKS));
            }
        }
        s
    }

    /// How to write a visible identity (sometimes only the name, sometimes with the module).
    pub(super) fn identity_ref(&mut self) -> String {
        let mods = self.visible_modules();
        let m = *self.rng.pick(&mods);
        if m == self.cur && self.rng.chance(1, 3) {
            format!("{}.{}", self.plans[m].name, IDENTITIES[m])
        } else {
            IDENTITIES[m].to_string()
        }
    }

    /// A quote line (with or without an identity; may span multiple lines).
    pub(super) fn quote_stmt(&mut self) {
        let who = if self.rng.chance(2, 3) {
            self.identity_ref()
        } else {
            String::new()
        };
        let pieces = 1 + self.rng.below(3);
        let mut text = String::new();
        for i in 0..pieces {
            if i > 0 {
                // Leading spaces and tabs of continuation lines are removed
                text.push('\n');
                text.push_str(["  ", "\t", "     "][self.rng.below(3)]);
            }
            text.push_str(&self.quote_inner());
        }
        let id = self.maybe_id();
        let ind = self.indent();
        writeln!(self.out, "{ind}{who}「{text}」{id}").unwrap();
    }

    /// `choice` (`if`, `once`, stable IDs, bodies that are one-line blocks or single statements, nesting).
    /// At least one option can always be chosen.
    pub(super) fn choice_stmt(&mut self, depth: usize) {
        self.line("choice {");
        self.scopes.push(Vec::new());
        let n = 1 + self.rng.below(4);
        let always = self.rng.below(n);
        for i in 0..n {
            let text = self.quote_inner();
            let mut head = format!("「{text}」");
            let free = i == always;
            let once = !free && self.rng.chance(1, 3);
            if once {
                head.push_str(" once");
            }
            if !free && self.rng.chance(1, 3) {
                let c = self.bool_expr(2);
                write!(head, " if {c}").unwrap();
            }
            if once || self.rng.chance(1, 2) {
                self.next_id += 1;
                write!(head, " #i{}", self.next_id).unwrap();
            }
            match self.rng.below(4) {
                0 => {
                    self.line(&format!("{head} => {{"));
                    self.block(depth);
                    self.line("}");
                }
                1 => {
                    let t = self.plain_text();
                    self.line(&format!("{head} => {t}"));
                }
                2 => {
                    let e = self.int_expr(1);
                    self.line(&format!("{head} => {{ fuel -= ({e}) % 3; 選んだ }}"));
                }
                _ => match self.pick_jump() {
                    Some(l) if self.ret.is_none() => self.line(&format!("{head} => jump {l}")),
                    _ => self.line(&format!("{head} => 選んだ")),
                },
            }
        }
        self.scopes.pop();
        self.line("}");
    }

    /// An expression of type `k`.
    pub(super) fn expr_of(&mut self, k: Kind, depth: usize) -> String {
        match k {
            Kind::Int => self.int_expr(depth),
            Kind::Float => self.float_expr(depth),
            Kind::Bool => self.bool_expr(depth),
            Kind::Str => self.str_expr(depth),
        }
    }

    /// The expression to `return` (integers get `% 1000` so they do not grow too large).
    pub(super) fn returned(&mut self, k: Kind) -> String {
        match k {
            Kind::Int => format!("({}) % 1000", self.int_expr(1)),
            other => self.expr_of(other, 1),
        }
    }

    /// How to write a referenced label. In the same module, sometimes only the name, sometimes with the module.
    pub(super) fn label_path(&mut self, module: usize, name: &str) -> String {
        if module == self.cur && self.rng.chance(1, 2) {
            name.to_string()
        } else {
            format!("{}.{name}", self.plans[module].name)
        }
    }

    /// Modules that can be referred to (itself, and those `import`ed).
    pub(super) fn visible_modules(&self) -> Vec<usize> {
        let mut v = vec![self.cur];
        v.extend(self.plans[self.cur].imports.iter().copied());
        v
    }

    /// Labels that can be `jump`ed to.
    pub(super) fn pick_jump(&mut self) -> Option<String> {
        let mut all = Vec::new();
        for m in self.visible_modules() {
            for l in &self.plans[m].labels {
                if l.plain() {
                    all.push((m, l.name.clone()));
                }
            }
        }
        if all.is_empty() {
            return None;
        }
        let (m, name) = self.rng.pick(&all).clone();
        Some(self.label_path(m, &name))
    }

    /// Chooses one label that can be `call`ed (with `want_value`, one with a return type).
    pub(super) fn pick_callee(&mut self, want_value: Option<Kind>) -> Option<(String, LabelPlan)> {
        let mut all = Vec::new();
        for m in self.visible_modules() {
            for l in &self.plans[m].labels {
                if want_value.is_none_or(|k| l.ret == Some(k)) {
                    all.push((m, l.clone()));
                }
            }
        }
        if all.is_empty() {
            return None;
        }
        let (m, l) = self.rng.pick(&all).clone();
        let path = self.label_path(m, &l.name);
        Some((path, l))
    }

    pub(super) fn call_args(&mut self, params: &[Kind]) -> String {
        let args: Vec<String> = params.iter().map(|k| self.expr_of(*k, 1)).collect();
        args.join(", ")
    }

    /// `call` (discarding the return value, receiving it with `let`, assigning it to a variable).
    pub(super) fn call_stmt(&mut self) {
        match self.rng.below(3) {
            0 => {
                let Some((path, l)) = self.pick_callee(None) else {
                    return;
                };
                let args = self.call_args(&l.params);
                self.line(&format!("call {path}({args})"));
            }
            1 => {
                let Some((path, l)) = self.pick_callee(None) else {
                    return;
                };
                let Some(kind) = l.ret else {
                    let args = self.call_args(&l.params);
                    self.line(&format!("call {path}({args})"));
                    return;
                };
                let args = self.call_args(&l.params);
                self.next_local += 1;
                let name = format!("v{}", self.next_local);
                if self.rng.chance(1, 2) {
                    self.line(&format!(
                        "let {name}: {} = call {path}({args})",
                        kind_name(kind)
                    ));
                } else {
                    self.line(&format!("let {name} = call {path}({args})"));
                }
                self.scopes.last_mut().unwrap().push((name.clone(), kind));
                if kind == Kind::Float {
                    self.clamp_float(&name);
                }
            }
            _ => {
                // Assignment to an existing variable
                let all: Vec<(String, Kind)> = self.scopes.iter().flatten().cloned().collect();
                let (name, kind) = self.rng.pick(&all).clone();
                if name == "fuel" || name.ends_with(".fuel") || name.starts_with("lp") {
                    return;
                }
                let Some((path, l)) = self.pick_callee(Some(kind)) else {
                    return;
                };
                let args = self.call_args(&l.params);
                self.line(&format!("{name} = call {path}({args})"));
                if kind == Kind::Float {
                    self.clamp_float(&name);
                }
            }
        }
    }

    /// Keeps floating point variables within ±1000 (so expressions do not grow too large).
    pub(super) fn clamp_float(&mut self, name: &str) {
        self.line(&format!("if {name} > 1000.0 {{"));
        self.line(&format!("  {name} = 1000.0"));
        self.line("}");
        self.line(&format!("if {name} < -1000.0 {{"));
        self.line(&format!("  {name} = -1000.0"));
        self.line("}");
    }

    pub(super) fn assign(&mut self) {
        let all: Vec<(String, Kind)> = self.scopes.iter().flatten().cloned().collect();
        let (name, kind) = self.rng.pick(&all).clone();
        // Do not rewrite `fuel` or the counter variables of `while` (so it always terminates)
        if name == "fuel" || name.ends_with(".fuel") || name.starts_with("lp") {
            return;
        }
        match kind {
            Kind::Int => match self.rng.below(7) {
                0 => {
                    let e = self.int_expr(2);
                    self.line(&format!("{name} = ({e}) % 1000"));
                }
                1 => {
                    let c = self.rng.range(0, 20);
                    self.line(&format!("{name} += {c}"));
                }
                2 => {
                    let c = self.rng.range(0, 20);
                    self.line(&format!("{name} -= {c}"));
                }
                3 => {
                    let c = self.rng.range(-3, 5);
                    self.line(&format!("{name} *= {c}"));
                    self.line(&format!("{name} %= 1000"));
                }
                4 => {
                    let c = self.rng.range(2, 9);
                    self.line(&format!("{name} /= {c}"));
                }
                5 => {
                    let c = self.rng.range(2, 50);
                    self.line(&format!("{name} %= {c}"));
                }
                _ => {
                    let e = self.int_expr(1);
                    self.line(&format!("{name} += ({e}) % 10"));
                }
            },
            Kind::Float => {
                match self.rng.below(5) {
                    0 => {
                        let e = self.float_expr(2);
                        self.line(&format!("{name} = {e}"));
                    }
                    1 => {
                        let e = self.float_expr(1);
                        self.line(&format!("{name} += {e}"));
                    }
                    2 => {
                        let e = self.float_expr(1);
                        self.line(&format!("{name} -= {e}"));
                    }
                    3 => {
                        let c = self.rng.range(0, 9);
                        self.line(&format!("{name} *= {c}.5"));
                    }
                    _ => {
                        let c = self.rng.range(1, 9);
                        self.line(&format!("{name} /= {c}.25"));
                    }
                }
                self.clamp_float(&name);
            }
            Kind::Bool => {
                let e = self.bool_expr(2);
                self.line(&format!("{name} = {e}"));
            }
            Kind::Str => {
                let e = self.str_expr(2);
                self.line(&format!("{name} = {e}"));
            }
        }
    }

    pub(super) fn block(&mut self, depth: usize) {
        self.scopes.push(Vec::new());
        for _ in 0..self.rng.below(5) {
            self.stmt(depth.saturating_sub(1));
        }
        self.scopes.pop();
    }

    /// A `while` that always terminates by counting (1 to 3 iterations; also places `break` / `continue`).
    pub(super) fn while_stmt(&mut self, depth: usize) {
        self.next_local += 1;
        let counter = format!("lp{}", self.next_local);
        let n = self.rng.range(1, 3);
        self.line(&format!("let {counter} = 0"));
        self.scopes
            .last_mut()
            .unwrap()
            .push((counter.clone(), Kind::Int));
        self.line(&format!("while {counter} < {n} {{"));
        self.scopes.push(Vec::new());
        self.loop_depth += 1;
        self.line(&format!("{counter} += 1"));
        for _ in 0..self.rng.below(4) {
            self.stmt(depth.saturating_sub(1));
        }
        self.loop_depth -= 1;
        self.scopes.pop();
        self.line("}");
    }

    pub(super) fn if_stmt(&mut self, depth: usize) {
        let c = self.bool_expr(2);
        self.line(&format!("if {c} {{"));
        self.block(depth);
        let mut closed = false;
        while !closed {
            match self.rng.below(4) {
                0 => {
                    let c = self.bool_expr(1);
                    self.line(&format!("}} else if {c} {{"));
                    self.block(depth);
                }
                1 => {
                    self.line("} else {");
                    self.block(depth);
                    self.line("}");
                    closed = true;
                }
                _ => {
                    self.line("}");
                    closed = true;
                }
            }
        }
    }
}

pub(super) const MODULE_NAMES: &[&str] = &["main", "alpha", "ベータ"];
/// Identity names per module.
pub(super) const IDENTITIES: &[&str] = &["案内人", "村長", "師"];
/// Fragments usable in the contents of quotes and options (`「」` and `=` are excluded, because they change how the
/// boundaries are decided).
pub(super) const QUOTE_CHUNKS: &[&str] = &[
    "あ",
    "いう",
    "テスト",
    "、",
    "。",
    "a b",
    "!",
    "?",
    "…",
    "𠮷",
    "\\「",
    "\\」",
    "\\$",
    "\\\\",
    "\\#",
    "(",
    ")",
];
pub(super) const PLAIN_LABELS: &[&str] = &["l1", "場面", "l3"];
pub(super) const CALL_LABELS: &[&str] = &["f1", "呼ぶ", "f3", "f4"];
pub(super) const KINDS: &[Kind] = &[Kind::Int, Kind::Float, Kind::Bool, Kind::Str];

/// Makes the plan of the modules.
pub(super) fn plan_modules(rng: &mut Rng, n: usize) -> Vec<ModPlan> {
    let mut plans = Vec::new();
    for (m, module_name) in MODULE_NAMES[..n].iter().enumerate() {
        let mut globals = Vec::new();
        for (names, kind) in [
            (INT_GLOBALS, Kind::Int),
            (FLOAT_GLOBALS, Kind::Float),
            (BOOL_GLOBALS, Kind::Bool),
            (STR_GLOBALS, Kind::Str),
        ] {
            for name in names {
                if rng.chance(2, 3) {
                    globals.push((name.to_string(), kind, rng.chance(1, 4)));
                }
            }
        }
        let imports = (0..n).filter(|&o| o != m && rng.chance(2, 3)).collect();
        let mut labels = Vec::new();
        let plain = if m == 0 {
            rng.below(3)
        } else {
            1 + rng.below(2)
        };
        for name in &PLAIN_LABELS[..plain] {
            labels.push(LabelPlan {
                name: name.to_string(),
                params: Vec::new(),
                ret: None,
            });
        }
        let called = rng.below(4);
        for name in &CALL_LABELS[..called] {
            let params = (0..rng.below(3)).map(|_| *rng.pick(KINDS)).collect();
            let ret = if rng.chance(1, 2) {
                Some(*rng.pick(KINDS))
            } else {
                None
            };
            labels.push(LabelPlan {
                name: name.to_string(),
                params,
                ret,
            });
        }
        plans.push(ModPlan {
            name: module_name.to_string(),
            imports,
            globals,
            labels,
        });
    }
    // The start label
    plans[0].labels.insert(
        0,
        LabelPlan {
            name: "start".to_string(),
            params: Vec::new(),
            ret: None,
        },
    );
    plans
}

/// Makes a valid script with `n` modules (`(module name, source)`). Every chain of `jump`s and `call`s always terminates
/// thanks to each module's `fuel`. It starts at `main.start`.
pub(super) fn gen_program(rng: &mut Rng, n: usize) -> Vec<(String, String)> {
    let plans = plan_modules(rng, n);
    let mut out = Vec::new();
    let mut next_local = 0;
    for m in 0..n {
        let plan = plans[m].clone();
        let mut g = Gen {
            rng,
            out: String::new(),
            scopes: vec![Vec::new()],
            next_local,
            next_id: 0,
            plans: plans.clone(),
            cur: m,
            ret: None,
            callable: false,
            loop_depth: 0,
            cur_label: String::new(),
        };
        for &i in &plan.imports {
            writeln!(g.out, "import {}", plans[i].name).unwrap();
        }
        // Each module declares identities with names of its own (names do not collide, so they can be written by name alone from
        // anywhere)
        writeln!(g.out, "identity {0} = \"{0}\"", IDENTITIES[m]).unwrap();
        g.out.push_str("save var fuel: int = 25\n");
        g.scopes[0].push(("fuel".to_string(), Kind::Int));
        for (name, kind, persistent) in &plan.globals {
            let init = match kind {
                Kind::Int => g.rng.range(-5, 30).to_string(),
                Kind::Float => format!("{}.{}", g.rng.range(-5, 30), g.rng.range(0, 99)),
                Kind::Bool => g.rng.chance(1, 2).to_string(),
                Kind::Str => g.rng.pick(STR_LITERALS).to_string(),
            };
            let decl = if *persistent { "persistent" } else { "save" };
            writeln!(g.out, "{decl} var {name}: {} = {init}", kind_name(*kind)).unwrap();
            g.scopes[0].push((name.clone(), *kind));
            // It can also be read and written with its own module's name
            if g.rng.chance(1, 4) {
                g.scopes[0].push((format!("{}.{name}", plan.name), *kind));
            }
        }
        for &i in &plan.imports {
            for (name, kind, _) in &plans[i].globals {
                if g.rng.chance(1, 2) {
                    g.scopes[0].push((format!("{}.{name}", plans[i].name), *kind));
                }
            }
        }
        g.out.push('\n');
        for label in &plan.labels {
            g.next_id = 0;
            g.cur_label = label.name.clone();
            g.ret = label.ret;
            g.callable = !label.plain();
            let params: Vec<(String, Kind)> = label
                .params
                .iter()
                .enumerate()
                .map(|(i, k)| (format!("a{i}"), *k))
                .collect();
            let header: Vec<String> = params
                .iter()
                .map(|(n, k)| format!("{n}: {}", kind_name(*k)))
                .collect();
            let mut head = format!("label {}", label.name);
            if g.callable {
                write!(head, "({})", header.join(", ")).unwrap();
                if let Some(k) = label.ret {
                    write!(head, " -> {}", kind_name(k)).unwrap();
                }
            }
            writeln!(g.out, "{head} {{").unwrap();
            g.scopes.push(params);
            g.line("fuel -= 1");
            g.line("if fuel <= 0 {");
            g.line("  end");
            g.line("}");
            for _ in 0..(2 + g.rng.below(9)) {
                g.stmt(2);
            }
            if g.callable {
                match label.ret {
                    Some(k) => {
                        let v = g.returned(k);
                        g.line(&format!("return {v}"));
                    }
                    None => g.line("return"),
                }
            }
            g.scopes.pop();
            g.out.push_str("}\n\n");
        }
        next_local = g.next_local;
        out.push((plan.name.clone(), g.out));
    }
    out
}

/// Makes a valid (single-module) script. Includes `call` and labels with parameters and return values.
pub(super) fn gen_script(rng: &mut Rng) -> String {
    gen_program(rng, 1).remove(0).1
}

pub(super) fn compile_ok(src: &str, reg: &Registry) -> Program {
    match compile(src, "main", reg) {
        Ok(p) => p,
        Err(d) => panic!(
            "生成した台本がコンパイルできない: {}\n{src}",
            show_diags(&d)
        ),
    }
}

pub(super) fn show_diags(ds: &[Diagnostic]) -> String {
    ds.iter()
        .map(|d| format!("{}:{} {}", d.line, d.col, d.code.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}
