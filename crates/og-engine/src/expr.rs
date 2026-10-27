//! The expression language. The syntax follows QGIS: fields are written by name or in
//! "double quotes", text in 'single quotes'. Examples:
//!
//! ```text
//! population >= 10000 AND municipality <> 'Mark'
//! round("area m2" / 10000, 1)
//! lpad(to_string(code), 4, '0') || '-' || upper(name)
//! ```

use crate::geom;
use crate::model::{err, Feature, GbError, Result, Value};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64, bool),
    Str(String),
    Ident(String),
    QIdent(String),
    Var(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn lex(src: &str) -> Result<Vec<(Tok, usize)>> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let start = i;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let mut is_int = true;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                is_int = false;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    is_int = false;
                    i = j;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = chars[start..i].iter().collect();
            let v: f64 = text.parse().map_err(|_| GbError(format!("\"{}\" is not a number.", text)))?;
            out.push((Tok::Num(v, is_int && text.parse::<i64>().is_ok()), start));
            continue;
        }
        if c == '\'' || c == '"' {
            let mut s = String::new();
            i += 1;
            loop {
                match chars.get(i) {
                    None => return err(format!("The quote at character {} is never closed.", start + 1)),
                    Some(&d) if d == c => {
                        if chars.get(i + 1) == Some(&c) {
                            s.push(c);
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    }
                    Some(&d) => {
                        s.push(d);
                        i += 1;
                    }
                }
            }
            out.push((if c == '\'' { Tok::Str(s) } else { Tok::QIdent(s) }, start));
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '$' {
            i += 1;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            out.push((if c == '$' { Tok::Var(text[1..].to_lowercase()) } else { Tok::Ident(text) }, start));
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        let op2 = ["||", "<=", ">=", "<>", "!=", "==", "//"].iter().find(|o| **o == two);
        if let Some(o) = op2 {
            out.push((Tok::Op(o), start));
            i += 2;
            continue;
        }
        let tok = match c {
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            ',' => Tok::Comma,
            '+' => Tok::Op("+"),
            '-' => Tok::Op("-"),
            '*' => Tok::Op("*"),
            '/' => Tok::Op("/"),
            '%' => Tok::Op("%"),
            '^' => Tok::Op("^"),
            '=' => Tok::Op("="),
            '<' => Tok::Op("<"),
            '>' => Tok::Op(">"),
            _ => return err(format!("Unexpected character \"{}\" at character {}.", c, start + 1)),
        };
        out.push((tok, start));
        i += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    IntDiv,
    Mod,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Func {
    Round,
    Abs,
    Sqrt,
    Floor,
    Ceil,
    Min,
    Max,
    Upper,
    Lower,
    Length,
    Trim,
    Substr,
    Replace,
    Left,
    Right,
    Lpad,
    Rpad,
    Concat,
    Coalesce,
    If,
    ToInt,
    ToReal,
    ToString,
    Now,
    Initcap,
    FormatTime,
    Epoch,
    Power,
    Exp,
    Ln,
    Log10,
    Log,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Pi,
    Radians,
    Degrees,
    Clamp,
    Strpos,
    StartsWith,
    EndsWith,
    NullIf,
    RegexpReplace,
    RegexpMatch,
    RegexpSubstr,
    Year,
    Month,
    Day,
    Hour,
    Minute,
    Second,
    DayOfWeek,
    Week,
    DateAdd,
    DateDiff,
}

/// (name, function, fewest arguments, most arguments, description)
const FUNCS: &[(&str, Func, usize, usize, &str)] = &[
    ("round", Func::Round, 1, 2, "round(number, decimals) rounds"),
    ("abs", Func::Abs, 1, 1, "abs(number) gives the absolute value"),
    ("sqrt", Func::Sqrt, 1, 1, "sqrt(number) gives the square root"),
    ("floor", Func::Floor, 1, 1, "floor(number) rounds down"),
    ("ceil", Func::Ceil, 1, 1, "ceil(number) rounds up"),
    ("min", Func::Min, 1, 99, "min(a, b, …) gives the smallest number"),
    ("max", Func::Max, 1, 99, "max(a, b, …) gives the largest number"),
    ("upper", Func::Upper, 1, 1, "upper(text) gives upper case"),
    ("lower", Func::Lower, 1, 1, "lower(text) gives lower case"),
    ("initcap", Func::Initcap, 1, 1, "initcap(text) makes every word begin with a capital: Lilla Edet"),
    ("length", Func::Length, 1, 1, "length(text) gives the number of characters"),
    ("trim", Func::Trim, 1, 1, "trim(text) removes whitespace at both ends"),
    ("substr", Func::Substr, 2, 3, "substr(text, start, length) gives part of the text, start counts from 1"),
    ("replace", Func::Replace, 3, 3, "replace(text, search, replacement)"),
    ("left", Func::Left, 2, 2, "left(text, n) gives the first n characters"),
    ("right", Func::Right, 2, 2, "right(text, n) gives the last n characters"),
    ("lpad", Func::Lpad, 3, 3, "lpad(text, width, fill) pads on the left"),
    ("rpad", Func::Rpad, 3, 3, "rpad(text, width, fill) pads on the right"),
    ("concat", Func::Concat, 1, 99, "concat(a, b, …) joins text, null counts as empty"),
    ("coalesce", Func::Coalesce, 1, 99, "coalesce(a, b, …) gives the first value that is not null"),
    ("if", Func::If, 3, 3, "if(condition, if_true, if_false)"),
    ("to_int", Func::ToInt, 1, 1, "to_int(value) converts to an integer"),
    ("to_real", Func::ToReal, 1, 1, "to_real(value) converts to a decimal number"),
    ("to_string", Func::ToString, 1, 1, "to_string(value) converts to text"),
    ("now", Func::Now, 0, 1, "now(format) gives the date and time of the run, such as now('%Y-%m-%d') for 2026-10-15"),
    ("format_time", Func::FormatTime, 1, 3, "format_time(time, format, zone) writes a time from an API in Swedish time, or 'utc': ISO 8601 such as 2026-10-17T06:00:00Z, or seconds or milliseconds since 1970 as ArcGIS gives them. format_time(Changed, '%Y-%m-%d %H:%M')"),
    ("epoch", Func::Epoch, 1, 1, "epoch(time) gives seconds since 1970 for a time in ISO 8601, to count with: (epoch(end) - epoch(start)) / 60 is minutes"),
    ("power", Func::Power, 2, 2, "power(number, exponent), the same as number ^ exponent"),
    ("exp", Func::Exp, 1, 1, "exp(number) gives e raised to the number"),
    ("ln", Func::Ln, 1, 1, "ln(number) gives the natural logarithm"),
    ("log10", Func::Log10, 1, 1, "log10(number) gives the logarithm with base 10"),
    ("log", Func::Log, 2, 2, "log(base, number) gives the logarithm with that base"),
    ("sin", Func::Sin, 1, 1, "sin(angle) of an angle in radians; sin(radians(30)) for degrees"),
    ("cos", Func::Cos, 1, 1, "cos(angle) of an angle in radians"),
    ("tan", Func::Tan, 1, 1, "tan(angle) of an angle in radians"),
    ("asin", Func::Asin, 1, 1, "asin(number) gives the angle in radians"),
    ("acos", Func::Acos, 1, 1, "acos(number) gives the angle in radians"),
    ("atan", Func::Atan, 1, 1, "atan(number) gives the angle in radians"),
    ("atan2", Func::Atan2, 2, 2, "atan2(dy, dx) gives the angle in radians of a direction"),
    ("pi", Func::Pi, 0, 0, "pi() gives 3.14159…"),
    ("radians", Func::Radians, 1, 1, "radians(degrees) converts degrees to radians"),
    ("degrees", Func::Degrees, 1, 1, "degrees(radians) converts radians to degrees"),
    ("clamp", Func::Clamp, 3, 3, "clamp(minimum, number, maximum) keeps the number within the two"),
    ("strpos", Func::Strpos, 2, 2, "strpos(text, search) gives where the search begins, counted from 1, or 0"),
    ("starts_with", Func::StartsWith, 2, 2, "starts_with(text, beginning) is true when the text begins so"),
    ("ends_with", Func::EndsWith, 2, 2, "ends_with(text, end) is true when the text ends so"),
    ("nullif", Func::NullIf, 2, 2, "nullif(value, other) gives null when the two are equal, else the value: nullif(code, '-')"),
    ("regexp_replace", Func::RegexpReplace, 3, 3, "regexp_replace(text, pattern, replacement) replaces what a regular expression finds; \\1 in the replacement is the first group"),
    ("regexp_match", Func::RegexpMatch, 2, 2, "regexp_match(text, pattern) gives where the regular expression is first found, counted from 1, or 0"),
    ("regexp_substr", Func::RegexpSubstr, 2, 2, "regexp_substr(text, pattern) gives what the regular expression finds, or its first group: regexp_substr(address, '(\\d{3} ?\\d{2})')"),
    ("year", Func::Year, 1, 1, "year(time) of a date or time, in Swedish time when the zone is given"),
    ("month", Func::Month, 1, 1, "month(time), 1 to 12"),
    ("day", Func::Day, 1, 1, "day(time), the day of the month"),
    ("hour", Func::Hour, 1, 1, "hour(time), 0 to 23"),
    ("minute", Func::Minute, 1, 1, "minute(time), 0 to 59"),
    ("second", Func::Second, 1, 1, "second(time), 0 to 59"),
    ("day_of_week", Func::DayOfWeek, 1, 1, "day_of_week(time), 1 for Monday to 7 for Sunday"),
    ("week", Func::Week, 1, 1, "week(time) gives the week number as in Sweden (ISO 8601)"),
    ("date_add", Func::DateAdd, 3, 3, "date_add(time, amount, unit) adds 'seconds', 'minutes', 'hours', 'days' or 'weeks': date_add('2026-10-21T08:00', 15, 'minutes')"),
    ("date_diff", Func::DateDiff, 2, 3, "date_diff(end, start, unit) gives the time between, in 'seconds', 'minutes', 'hours' or 'days' (the default)"),
];

/// Every word with a capital first letter and the rest small. A letter after anything that
/// is not a letter or a digit begins a word, so Dals-ed becomes Dals-Ed. An apostrophe does
/// not, so o'neill becomes O'neill.
pub fn initcap(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = c != '\'' && c != '’';
        }
    }
    out
}

/// The functions with one line of help each, for the user interface and the documentation.
pub fn function_help() -> Vec<&'static str> {
    FUNCS.iter().map(|f| f.4).collect()
}

#[derive(Debug, Clone)]
enum Expr {
    Lit(Value),
    Field(usize),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    IsNull(Box<Expr>, bool),
    In(Box<Expr>, Vec<Expr>, bool),
    Like(Box<Expr>, Box<Expr>, bool, bool),
    Call(Func, Vec<Expr>),
    /// CASE WHEN … THEN … ELSE … END
    Case(Vec<(Expr, Expr)>, Option<Box<Expr>>),
    Area,
    Length,
    Perimeter,
    X,
    Y,
    Z,
    Id,
    NumPoints,
    Bound(usize),
}

/// The variables, with one line of help each.
const VARS: &[(&str, &str)] = &[
    ("$area", "$area is the area in square metres"),
    ("$length", "$length is the length of a line in metres, or the perimeter of an area"),
    ("$perimeter", "$perimeter is the perimeter of an area in metres, null for what is not an area"),
    ("$x", "$x is the x coordinate of a point; of anything else, null"),
    ("$y", "$y is the y coordinate of a point"),
    ("$z", "$z is the height of a point, when it has one"),
    ("$xmin", "$xmin, $ymin, $xmax and $ymax are the sides of the bounding box"),
    ("$num_points", "$num_points is the number of corners"),
    ("$id", "$id is the number of the feature in the stream, from 1"),
];

/// The variables with one line of help each, for the user interface.
pub fn variable_help() -> Vec<&'static str> {
    VARS.iter().map(|v| v.1).collect()
}

/// The names of the functions, for the user interface.
pub fn function_names() -> Vec<&'static str> {
    FUNCS.iter().map(|f| f.0).collect()
}

struct Parser<'a> {
    toks: Vec<(Tok, usize)>,
    pos: usize,
    fields: &'a [String],
    src_len: usize,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.0)
    }
    fn at(&self) -> usize {
        self.toks.get(self.pos).map(|t| t.1 + 1).unwrap_or(self.src_len + 1)
    }
    fn kw(&self, word: &str) -> bool {
        self.kw_at(self.pos, word)
    }
    fn kw_at(&self, pos: usize, word: &str) -> bool {
        matches!(self.toks.get(pos), Some((Tok::Ident(s), _)) if s.eq_ignore_ascii_case(word))
    }
    fn op(&mut self, ops: &[&'static str]) -> Option<&'static str> {
        if let Some(Tok::Op(o)) = self.peek() {
            if let Some(found) = ops.iter().find(|x| *x == o) {
                self.pos += 1;
                return Some(found);
            }
        }
        None
    }
    fn expect_rparen(&mut self) -> Result<()> {
        if self.peek() == Some(&Tok::RParen) {
            self.pos += 1;
            Ok(())
        } else {
            err(format!("A closing parenthesis is missing at character {}.", self.at()))
        }
    }
    fn field(&self, name: &str) -> Result<Expr> {
        match self.fields.iter().position(|f| f == name) {
            Some(i) => Ok(Expr::Field(i)),
            None => err(format!(
                "There is no attribute \"{}\". Available: {}.",
                name,
                if self.fields.is_empty() { "no attributes".to_string() } else { self.fields.join(", ") }
            )),
        }
    }

    fn or(&mut self) -> Result<Expr> {
        let mut left = self.and()?;
        while self.kw("or") {
            self.pos += 1;
            left = Expr::Bin(BinOp::Or, Box::new(left), Box::new(self.and()?));
        }
        Ok(left)
    }
    fn and(&mut self) -> Result<Expr> {
        let mut left = self.not()?;
        while self.kw("and") {
            self.pos += 1;
            left = Expr::Bin(BinOp::And, Box::new(left), Box::new(self.not()?));
        }
        Ok(left)
    }
    fn not(&mut self) -> Result<Expr> {
        if self.kw("not") {
            self.pos += 1;
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Expr> {
        let mut left = self.concat()?;
        loop {
            if let Some(o) = self.op(&["=", "==", "!=", "<>", "<=", ">=", "<", ">"]) {
                let op = match o {
                    "=" | "==" => BinOp::Eq,
                    "!=" | "<>" => BinOp::Ne,
                    "<" => BinOp::Lt,
                    "<=" => BinOp::Le,
                    ">" => BinOp::Gt,
                    _ => BinOp::Ge,
                };
                left = Expr::Bin(op, Box::new(left), Box::new(self.concat()?));
                continue;
            }
            if self.kw("is") {
                self.pos += 1;
                let neg = self.kw("not");
                if neg {
                    self.pos += 1;
                }
                if !self.kw("null") {
                    return err(format!("IS must be followed by NULL or NOT NULL (character {}).", self.at()));
                }
                self.pos += 1;
                left = Expr::IsNull(Box::new(left), neg);
                continue;
            }
            let neg = self.kw("not") && (self.kw_at(self.pos + 1, "in") || self.kw_at(self.pos + 1, "like") || self.kw_at(self.pos + 1, "ilike"));
            let p = self.pos + neg as usize;
            if self.kw_at(p, "in") {
                self.pos = p + 1;
                if self.peek() != Some(&Tok::LParen) {
                    return err(format!("IN must be followed by a list in parentheses (character {}).", self.at()));
                }
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    items.push(self.or()?);
                    if self.peek() == Some(&Tok::Comma) {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                self.expect_rparen()?;
                left = Expr::In(Box::new(left), items, neg);
                continue;
            }
            if self.kw_at(p, "like") || self.kw_at(p, "ilike") {
                let insensitive = self.kw_at(p, "ilike");
                self.pos = p + 1;
                left = Expr::Like(Box::new(left), Box::new(self.concat()?), neg, insensitive);
                continue;
            }
            return Ok(left);
        }
    }
    fn concat(&mut self) -> Result<Expr> {
        let mut left = self.add()?;
        while self.op(&["||"]).is_some() {
            left = Expr::Bin(BinOp::Concat, Box::new(left), Box::new(self.add()?));
        }
        Ok(left)
    }
    fn add(&mut self) -> Result<Expr> {
        let mut left = self.mul()?;
        while let Some(o) = self.op(&["+", "-"]) {
            let op = if o == "+" { BinOp::Add } else { BinOp::Sub };
            left = Expr::Bin(op, Box::new(left), Box::new(self.mul()?));
        }
        Ok(left)
    }
    fn mul(&mut self) -> Result<Expr> {
        let mut left = self.unary()?;
        while let Some(o) = self.op(&["*", "/", "//", "%"]) {
            let op = match o {
                "*" => BinOp::Mul,
                "/" => BinOp::Div,
                "//" => BinOp::IntDiv,
                _ => BinOp::Mod,
            };
            left = Expr::Bin(op, Box::new(left), Box::new(self.unary()?));
        }
        Ok(left)
    }
    fn unary(&mut self) -> Result<Expr> {
        if self.op(&["-"]).is_some() {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.op(&["+"]).is_some() {
            return self.unary();
        }
        self.power()
    }
    /// a ^ b binds harder than the minus in front, as in QGIS: -2 ^ 2 is -4. 2 ^ 3 ^ 2 is 2 ^ 9.
    fn power(&mut self) -> Result<Expr> {
        let base = self.primary()?;
        if self.op(&["^"]).is_some() {
            return Ok(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(self.unary()?)));
        }
        Ok(base)
    }
    fn case(&mut self) -> Result<Expr> {
        let mut arms = Vec::new();
        while self.kw("when") {
            self.pos += 1;
            let cond = self.or()?;
            if !self.kw("then") {
                return err(format!("WHEN must be followed by THEN (character {}).", self.at()));
            }
            self.pos += 1;
            arms.push((cond, self.or()?));
        }
        if arms.is_empty() {
            return err(format!("CASE must be followed by WHEN (character {}).", self.at()));
        }
        let otherwise = if self.kw("else") {
            self.pos += 1;
            Some(Box::new(self.or()?))
        } else {
            None
        };
        if !self.kw("end") {
            return err(format!("CASE must end with END (character {}).", self.at()));
        }
        self.pos += 1;
        Ok(Expr::Case(arms, otherwise))
    }
    fn primary(&mut self) -> Result<Expr> {
        let at = self.at();
        let tok = match self.toks.get(self.pos) {
            Some(t) => t.0.clone(),
            None => return err("The expression ends too early."),
        };
        self.pos += 1;
        match tok {
            Tok::Num(v, true) => Ok(Expr::Lit(Value::Int(v as i64))),
            Tok::Num(v, false) => Ok(Expr::Lit(Value::Real(v))),
            Tok::Str(s) => Ok(Expr::Lit(Value::Text(s))),
            Tok::QIdent(s) => self.field(&s),
            Tok::Var(v) => match v.as_str() {
                "area" => Ok(Expr::Area),
                "length" => Ok(Expr::Length),
                "perimeter" => Ok(Expr::Perimeter),
                "x" => Ok(Expr::X),
                "y" => Ok(Expr::Y),
                "z" => Ok(Expr::Z),
                "id" => Ok(Expr::Id),
                "num_points" => Ok(Expr::NumPoints),
                "xmin" => Ok(Expr::Bound(0)),
                "ymin" => Ok(Expr::Bound(1)),
                "xmax" => Ok(Expr::Bound(2)),
                "ymax" => Ok(Expr::Bound(3)),
                _ => err(format!(
                    "Unknown variable ${}. Available: {}.",
                    v,
                    VARS.iter()
                        .map(|v| v.0)
                        .chain(["$xmin", "$ymin", "$xmax", "$ymax"])
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            },
            Tok::LParen => {
                let e = self.or()?;
                self.expect_rparen()?;
                Ok(e)
            }
            Tok::Ident(name) => {
                if self.peek() == Some(&Tok::LParen) {
                    let lname = name.to_lowercase();
                    let f = FUNCS.iter().find(|f| f.0 == lname).ok_or_else(|| {
                        GbError(format!("Unknown function {}(). Available: {}.", name, FUNCS.iter().map(|f| f.0).collect::<Vec<_>>().join(", ")))
                    })?;
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.or()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect_rparen()?;
                    if args.len() < f.2 || args.len() > f.3 {
                        return err(format!("{}() has the wrong number of arguments. {}.", f.0, capitalize(f.4)));
                    }
                    return Ok(Expr::Call(f.1, args));
                }
                match name.to_lowercase().as_str() {
                    "case" => self.case(),
                    "null" => Ok(Expr::Lit(Value::Null)),
                    "true" => Ok(Expr::Lit(Value::Bool(true))),
                    "false" => Ok(Expr::Lit(Value::Bool(false))),
                    "and" | "or" | "not" | "is" | "in" | "like" | "ilike" | "when" | "then" | "else" | "end" => {
                        err(format!("The word {} is in the wrong place (character {}).", name.to_uppercase(), at))
                    }
                    _ => self.field(&name),
                }
            }
            Tok::RParen => err(format!("Unexpected closing parenthesis at character {}.", at)),
            Tok::Comma => err(format!("Unexpected comma at character {}.", at)),
            Tok::Op(o) => err(format!("Unexpected \"{}\" at character {}.", o, at)),
        }
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A parsed expression, bound to a particular schema.
#[derive(Debug, Clone)]
pub struct Compiled {
    expr: Expr,
}

pub fn compile(src: &str, fields: &[String]) -> Result<Compiled> {
    if src.trim().is_empty() {
        return err("The expression is empty.");
    }
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0, fields, src_len: src.chars().count() };
    let expr = p.or()?;
    if p.pos < p.toks.len() {
        return err(format!("The expression cannot be read from character {} on.", p.at()));
    }
    Ok(Compiled { expr })
}

/// What an expression gets to know about the feature it is evaluated on.
pub struct Row<'a> {
    pub feature: &'a Feature,
    /// Where the feature is in its stream, from 0. $id is this plus one.
    pub index: usize,
    /// Whether the coordinate system of the stream is in degrees. `None` if unknown.
    pub geographic: Option<bool>,
}

impl Compiled {
    pub fn eval(&self, row: &Row) -> Result<Value> {
        eval(&self.expr, row)
    }
    /// True or false. Null counts as false.
    pub fn test(&self, row: &Row) -> Result<bool> {
        Ok(truth(&eval(&self.expr, row)?).unwrap_or(false))
    }
}

#[derive(Clone, Copy)]
enum Num {
    I(i64),
    R(f64),
}
impl Num {
    fn f(self) -> f64 {
        match self {
            Num::I(i) => i as f64,
            Num::R(r) => r,
        }
    }
}

fn num(v: &Value) -> Result<Option<Num>> {
    Ok(Some(match v {
        Value::Null => return Ok(None),
        Value::Int(i) => Num::I(*i),
        Value::Bool(b) => Num::I(*b as i64),
        Value::Real(r) => Num::R(*r),
        Value::Text(s) => {
            let t = s.trim();
            if let Ok(i) = t.parse::<i64>() {
                Num::I(i)
            } else if let Some(r) = crate::model::parse_number(t) {
                Num::R(r)
            } else {
                return err(format!("Cannot calculate with the text '{}'.", s));
            }
        }
    }))
}

fn truth(v: &Value) -> Option<bool> {
    match v {
        Value::Null => None,
        Value::Bool(b) => Some(*b),
        Value::Int(i) => Some(*i != 0),
        Value::Real(r) => Some(*r != 0.0),
        Value::Text(s) => Some(!s.is_empty()),
    }
}

fn is_numeric(v: &Value) -> bool {
    matches!(v, Value::Int(_) | Value::Real(_) | Value::Bool(_))
}

/// Compares two values that are not null. Numbers compare as numbers, also against text that is a number.
fn compare(a: &Value, b: &Value) -> std::cmp::Ordering {
    let both_text = matches!((a, b), (Value::Text(_), Value::Text(_)));
    if !both_text && (is_numeric(a) || is_numeric(b)) {
        if let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) {
            return x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
        }
    }
    a.to_text().cmp(&b.to_text())
}

fn like(text: &[char], pat: &[char]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some('%') => (0..=text.len()).any(|i| like(&text[i..], &pat[1..])),
        Some('_') => !text.is_empty() && like(&text[1..], &pat[1..]),
        Some(c) => text.first() == Some(c) && like(&text[1..], &pat[1..]),
    }
}

fn eval(e: &Expr, row: &Row) -> Result<Value> {
    Ok(match e {
        Expr::Lit(v) => v.clone(),
        Expr::Field(i) => row.feature.attrs.get(*i).cloned().unwrap_or(Value::Null),
        Expr::Neg(a) => match num(&eval(a, row)?)? {
            None => Value::Null,
            Some(Num::I(i)) => Value::Int(-i),
            Some(Num::R(r)) => Value::Real(-r),
        },
        Expr::Not(a) => truth(&eval(a, row)?).map_or(Value::Null, |b| Value::Bool(!b)),
        Expr::IsNull(a, neg) => Value::Bool(eval(a, row)?.is_null() != *neg),
        Expr::In(a, items, neg) => {
            let v = eval(a, row)?;
            if v.is_null() {
                return Ok(Value::Null);
            }
            let mut found = false;
            for it in items {
                let w = eval(it, row)?;
                if !w.is_null() && compare(&v, &w) == std::cmp::Ordering::Equal {
                    found = true;
                    break;
                }
            }
            Value::Bool(found != *neg)
        }
        Expr::Like(a, p, neg, insensitive) => {
            let (v, pat) = (eval(a, row)?, eval(p, row)?);
            if v.is_null() || pat.is_null() {
                return Ok(Value::Null);
            }
            let (mut t, mut q) = (v.to_text(), pat.to_text());
            if *insensitive {
                t = t.to_lowercase();
                q = q.to_lowercase();
            }
            let (t, q): (Vec<char>, Vec<char>) = (t.chars().collect(), q.chars().collect());
            Value::Bool(like(&t, &q) != *neg)
        }
        Expr::Area => match row.feature.geom.as_ref() {
            None => Value::Null,
            Some(g) => Value::Real(geom::area(g, need_geographic(row)?)),
        },
        Expr::Length => match row.feature.geom.as_ref() {
            None => Value::Null,
            Some(g) => Value::Real(geom::length(g, need_geographic(row)?)),
        },
        Expr::Perimeter => match row.feature.geom.as_ref() {
            Some(g) if geom::family(g) == geom::Family::Area => Value::Real(geom::length(g, need_geographic(row)?)),
            _ => Value::Null,
        },
        Expr::X | Expr::Y | Expr::Z => {
            let pt = match &row.feature.geom {
                Some(geo::Geometry::Point(p)) => Some(p.0),
                Some(geo::Geometry::MultiPoint(m)) if m.0.len() == 1 => Some(m.0[0].0),
                _ => None,
            };
            match (e, pt) {
                (_, None) => Value::Null,
                (Expr::X, Some(c)) => Value::Real(c.x),
                (Expr::Y, Some(c)) => Value::Real(c.y),
                _ => row.feature.z.as_ref().and_then(|z| z.first().copied()).filter(|z| z.is_finite()).map_or(Value::Null, Value::Real),
            }
        }
        Expr::Id => Value::Int(row.index as i64 + 1),
        Expr::NumPoints => match &row.feature.geom {
            None => Value::Null,
            Some(g) => {
                use geo::CoordsIter;
                Value::Int(g.coords_count() as i64)
            }
        },
        Expr::Bound(k) => {
            use geo::BoundingRect;
            match row.feature.geom.as_ref().and_then(|g| g.bounding_rect()) {
                None => Value::Null,
                Some(r) => Value::Real([r.min().x, r.min().y, r.max().x, r.max().y][*k]),
            }
        }
        Expr::Case(arms, otherwise) => {
            for (cond, then) in arms {
                if truth(&eval(cond, row)?) == Some(true) {
                    return eval(then, row);
                }
            }
            match otherwise {
                Some(o) => eval(o, row)?,
                None => Value::Null,
            }
        }
        Expr::Bin(op, a, b) => {
            // AND and OR follow three-valued logic and do not evaluate the right side needlessly.
            if matches!(op, BinOp::And | BinOp::Or) {
                let l = truth(&eval(a, row)?);
                let short = *op == BinOp::Or;
                if l == Some(short) {
                    return Ok(Value::Bool(short));
                }
                let r = truth(&eval(b, row)?);
                return Ok(match (l, r) {
                    (_, Some(x)) if x == short => Value::Bool(short),
                    (Some(_), Some(_)) => Value::Bool(!short),
                    _ => Value::Null,
                });
            }
            let (l, r) = (eval(a, row)?, eval(b, row)?);
            if l.is_null() || r.is_null() {
                return Ok(Value::Null);
            }
            match op {
                BinOp::Concat => Value::Text(l.to_text() + &r.to_text()),
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    use std::cmp::Ordering::*;
                    let o = compare(&l, &r);
                    Value::Bool(match op {
                        BinOp::Eq => o == Equal,
                        BinOp::Ne => o != Equal,
                        BinOp::Lt => o == Less,
                        BinOp::Le => o != Greater,
                        BinOp::Gt => o == Greater,
                        _ => o != Less,
                    })
                }
                _ => {
                    let (x, y) = match (num(&l)?, num(&r)?) {
                        (Some(x), Some(y)) => (x, y),
                        _ => return Ok(Value::Null),
                    };
                    arith(*op, x, y)
                }
            }
        }
        Expr::Call(f, args) => call(*f, args, row)?,
    })
}

fn need_geographic(row: &Row) -> Result<bool> {
    row.geographic
        .ok_or_else(|| GbError("It is not known whether the coordinate system is in metres or degrees, so area and length cannot be calculated.".into()))
}

fn arith(op: BinOp, x: Num, y: Num) -> Value {
    if let (Num::I(a), Num::I(b)) = (x, y) {
        let r = match op {
            BinOp::Add => a.checked_add(b),
            BinOp::Sub => a.checked_sub(b),
            BinOp::Mul => a.checked_mul(b),
            BinOp::IntDiv => {
                if b == 0 {
                    return Value::Null;
                }
                Some(a.div_euclid(b))
            }
            BinOp::Mod => {
                if b == 0 {
                    return Value::Null;
                }
                Some(a.rem_euclid(b))
            }
            BinOp::Pow if (0..=62).contains(&b) => a.checked_pow(b as u32),
            _ => None,
        };
        if let Some(v) = r {
            return Value::Int(v);
        }
    }
    let (a, b) = (x.f(), y.f());
    let v = match op {
        BinOp::Add => a + b,
        BinOp::Sub => a - b,
        BinOp::Mul => a * b,
        BinOp::Div => {
            if b == 0.0 {
                return Value::Null;
            }
            a / b
        }
        BinOp::IntDiv => {
            if b == 0.0 {
                return Value::Null;
            }
            (a / b).floor()
        }
        BinOp::Pow => a.powf(b),
        _ => {
            if b == 0.0 {
                return Value::Null;
            }
            a.rem_euclid(b)
        }
    };
    if v.is_finite() {
        Value::Real(v)
    } else {
        Value::Null
    }
}

fn call(f: Func, args: &[Expr], row: &Row) -> Result<Value> {
    // Functions that must not evaluate all their arguments up front.
    match f {
        Func::Coalesce => {
            for a in args {
                let v = eval(a, row)?;
                if !v.is_null() {
                    return Ok(v);
                }
            }
            return Ok(Value::Null);
        }
        Func::If => {
            let c = truth(&eval(&args[0], row)?).unwrap_or(false);
            return eval(&args[if c { 1 } else { 2 }], row);
        }
        _ => {}
    }
    let v: Vec<Value> = args.iter().map(|a| eval(a, row)).collect::<Result<_>>()?;
    if f == Func::Concat {
        return Ok(Value::Text(v.iter().map(|x| x.to_text()).collect()));
    }
    if v.iter().any(|x| x.is_null()) {
        return Ok(Value::Null);
    }
    let n = |i: usize| -> Result<f64> { Ok(num(&v[i])?.map(|x| x.f()).unwrap_or(0.0)) };
    let int = |i: usize| -> Result<i64> { Ok(n(i)?.round() as i64) };
    let real = |x: f64| if x.is_finite() { Value::Real(x) } else { Value::Null };
    let text = |i: usize| v[i].to_text();
    Ok(match f {
        Func::Round => {
            let d = if v.len() > 1 { int(1)? } else { 0 };
            if d <= 0 && v.len() == 1 {
                if let Value::Int(i) = v[0] {
                    return Ok(Value::Int(i));
                }
                let r = n(0)?.round();
                return Ok(if r.abs() < 9e15 { Value::Int(r as i64) } else { real(r) });
            }
            let k = 10f64.powi(d.clamp(-15, 15) as i32);
            real((n(0)? * k).round() / k)
        }
        Func::Abs => match num(&v[0])? {
            Some(Num::I(i)) => Value::Int(i.abs()),
            Some(Num::R(r)) => Value::Real(r.abs()),
            None => Value::Null,
        },
        Func::Sqrt => {
            let x = n(0)?;
            if x < 0.0 {
                Value::Null
            } else {
                real(x.sqrt())
            }
        }
        Func::Floor => Value::Int(n(0)?.floor() as i64),
        Func::Ceil => Value::Int(n(0)?.ceil() as i64),
        Func::Min | Func::Max => {
            let mut best = num(&v[0])?.unwrap();
            for x in &v[1..] {
                let c = num(x)?.unwrap();
                if (f == Func::Min && c.f() < best.f()) || (f == Func::Max && c.f() > best.f()) {
                    best = c;
                }
            }
            match best {
                Num::I(i) => Value::Int(i),
                Num::R(r) => Value::Real(r),
            }
        }
        Func::Upper => Value::Text(text(0).to_uppercase()),
        Func::Lower => Value::Text(text(0).to_lowercase()),
        Func::Length => Value::Int(text(0).chars().count() as i64),
        Func::Trim => Value::Text(text(0).trim().to_string()),
        Func::Substr => {
            let chars: Vec<char> = text(0).chars().collect();
            let start = (int(1)?.max(1) - 1) as usize;
            let len = if v.len() > 2 { int(2)?.max(0) as usize } else { usize::MAX };
            Value::Text(chars.iter().skip(start).take(len).collect())
        }
        Func::Replace => Value::Text(text(0).replace(&text(1), &text(2))),
        Func::Left => Value::Text(text(0).chars().take(int(1)?.max(0) as usize).collect()),
        Func::Right => {
            let chars: Vec<char> = text(0).chars().collect();
            let k = (int(1)?.max(0) as usize).min(chars.len());
            Value::Text(chars[chars.len() - k..].iter().collect())
        }
        Func::Lpad | Func::Rpad => {
            let s = text(0);
            let width = int(1)?.clamp(0, 10_000) as usize;
            let fill: Vec<char> = text(2).chars().collect();
            let have = s.chars().count();
            if fill.is_empty() || have >= width {
                Value::Text(s.chars().take(width).collect())
            } else {
                let pad: String = fill.iter().cycle().take(width - have).collect();
                Value::Text(if f == Func::Lpad { pad + &s } else { s + &pad })
            }
        }
        Func::ToInt => match num(&v[0])? {
            Some(Num::I(i)) => Value::Int(i),
            Some(Num::R(r)) => Value::Int(r.round() as i64),
            None => Value::Null,
        },
        Func::ToReal => real(n(0)?),
        Func::ToString => Value::Text(text(0)),
        Func::Initcap => Value::Text(initcap(&text(0))),
        Func::Now => {
            let format = if v.is_empty() { "%Y-%m-%d %H:%M".to_string() } else { text(0) };
            let now = chrono::Local::now();
            // A format chrono does not understand gives an error, not a panic.
            let mut out = String::new();
            use std::fmt::Write;
            if write!(out, "{}", now.format(&format)).is_err() {
                return err(format!("now: \"{}\" is not a date format. Use %Y for the year, %m the month, %d the day, %H:%M the time.", format));
            }
            Value::Text(out)
        }
        Func::FormatTime => {
            let format = if v.len() > 1 && v[1] != Value::Null { text(1) } else { "%Y-%m-%d %H:%M".to_string() };
            let zone = if v.len() > 2 { text(2).to_lowercase() } else { "swedish".to_string() };
            let Some(t) = moment(&v[0]) else { return Ok(Value::Null) };
            let shown = match t {
                Moment::At(utc) if zone == "utc" => utc,
                Moment::At(utc) => utc + chrono::Duration::hours(crate::sun::swedish_offset(utc)),
                // A time without a zone is written as it is.
                Moment::Local(t) => t,
            };
            let mut out = String::new();
            use std::fmt::Write;
            if write!(out, "{}", shown.format(&format)).is_err() {
                return err(format!("format_time: \"{}\" is not a date format. Use %Y for the year, %m the month, %d the day, %H:%M the time.", format));
            }
            Value::Text(out)
        }
        Func::Epoch => match moment(&v[0]) {
            Some(Moment::At(t)) | Some(Moment::Local(t)) => Value::Int(t.and_utc().timestamp()),
            None => Value::Null,
        },
        Func::Power => real(n(0)?.powf(n(1)?)),
        Func::Exp => real(n(0)?.exp()),
        Func::Ln => {
            let x = n(0)?;
            if x <= 0.0 {
                Value::Null
            } else {
                real(x.ln())
            }
        }
        Func::Log10 => {
            let x = n(0)?;
            if x <= 0.0 {
                Value::Null
            } else {
                real(x.log10())
            }
        }
        Func::Log => {
            let (b, x) = (n(0)?, n(1)?);
            if b <= 0.0 || b == 1.0 || x <= 0.0 {
                Value::Null
            } else {
                real(x.ln() / b.ln())
            }
        }
        Func::Sin => real(n(0)?.sin()),
        Func::Cos => real(n(0)?.cos()),
        Func::Tan => real(n(0)?.tan()),
        Func::Asin => real(n(0)?.asin()),
        Func::Acos => real(n(0)?.acos()),
        Func::Atan => real(n(0)?.atan()),
        Func::Atan2 => real(n(0)?.atan2(n(1)?)),
        Func::Pi => Value::Real(std::f64::consts::PI),
        Func::Radians => real(n(0)?.to_radians()),
        Func::Degrees => real(n(0)?.to_degrees()),
        Func::Clamp => {
            let (lo, x, hi) = (num(&v[0])?.unwrap(), num(&v[1])?.unwrap(), num(&v[2])?.unwrap());
            let pick = if x.f() < lo.f() {
                lo
            } else if x.f() > hi.f() {
                hi
            } else {
                x
            };
            match pick {
                Num::I(i) => Value::Int(i),
                Num::R(r) => Value::Real(r),
            }
        }
        Func::Strpos => {
            let (t, s) = (text(0), text(1));
            Value::Int(t.find(&s).map_or(0, |b| t[..b].chars().count() as i64 + 1))
        }
        Func::StartsWith => Value::Bool(text(0).starts_with(&text(1))),
        Func::EndsWith => Value::Bool(text(0).ends_with(&text(1))),
        Func::NullIf => {
            if compare(&v[0], &v[1]) == std::cmp::Ordering::Equal {
                Value::Null
            } else {
                v[0].clone()
            }
        }
        Func::RegexpReplace | Func::RegexpMatch | Func::RegexpSubstr => {
            let name = FUNCS.iter().find(|x| x.1 == f).map(|x| x.0).unwrap_or("regexp");
            let re = regex::Regex::new(&text(1)).map_err(|e| GbError(format!("{}: the pattern '{}' cannot be read: {}", name, text(1), e)))?;
            let t = text(0);
            match f {
                // \1 as in QGIS, and $1 as the regex crate writes it.
                Func::RegexpReplace => Value::Text(re.replace_all(&t, backslash_groups(&text(2)).as_str()).into_owned()),
                Func::RegexpMatch => Value::Int(re.find(&t).map_or(0, |m| t[..m.start()].chars().count() as i64 + 1)),
                _ => match re.captures(&t) {
                    None => Value::Null,
                    Some(c) => Value::Text(c.get(1).or_else(|| c.get(0)).map(|m| m.as_str().to_string()).unwrap_or_default()),
                },
            }
        }
        Func::Year | Func::Month | Func::Day | Func::Hour | Func::Minute | Func::Second | Func::DayOfWeek | Func::Week => {
            use chrono::{Datelike, Timelike};
            let Some(t) = moment(&v[0]).map(swedish) else { return Ok(Value::Null) };
            Value::Int(match f {
                Func::Year => t.year() as i64,
                Func::Month => t.month() as i64,
                Func::Day => t.day() as i64,
                Func::Hour => t.hour() as i64,
                Func::Minute => t.minute() as i64,
                Func::Second => t.second() as i64,
                Func::DayOfWeek => t.weekday().number_from_monday() as i64,
                _ => t.iso_week().week() as i64,
            })
        }
        Func::DateAdd => {
            let Some(t) = moment(&v[0]) else { return Ok(Value::Null) };
            let secs = (n(1)? * unit_seconds("date_add", &text(2))?).round() as i64;
            let d = chrono::Duration::seconds(secs);
            Value::Text(match t {
                Moment::At(u) => (u + d).format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                Moment::Local(l) => (l + d).format("%Y-%m-%dT%H:%M:%S").to_string(),
            })
        }
        Func::DateDiff => {
            let (Some(a), Some(b)) = (moment(&v[0]), moment(&v[1])) else { return Ok(Value::Null) };
            let unit = if v.len() > 2 { text(2) } else { "days".to_string() };
            let secs = (instant(a) - instant(b)).num_milliseconds() as f64 / 1000.0;
            real(secs / unit_seconds("date_diff", &unit)?)
        }
        Func::Concat | Func::Coalesce | Func::If => unreachable!(),
    })
}

/// \1 for a group in a replacement, as QGIS writes it, made into ${1} for the regex crate.
fn backslash_groups(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(d)) if d.is_ascii_digit() => {
                out.push_str(&format!("${{{}}}", d));
                chars.next();
            }
            ('$', _) => out.push_str("$$"),
            _ => out.push(c),
        }
    }
    out
}

/// How many seconds a unit of time is.
fn unit_seconds(func: &str, unit: &str) -> Result<f64> {
    Ok(match unit.trim().to_lowercase().trim_end_matches('s') {
        "second" | "sec" | "s" => 1.0,
        "minute" | "min" => 60.0,
        "hour" | "h" => 3600.0,
        "day" | "d" => 86_400.0,
        "week" => 604_800.0,
        _ => return err(format!("{}: '{}' is not a unit of time. Use 'seconds', 'minutes', 'hours', 'days' or 'weeks'.", func, unit)),
    })
}

/// A moment as a point on one time line, to count between: a time without a zone is
/// taken as it is.
fn instant(m: Moment) -> chrono::NaiveDateTime {
    match m {
        Moment::At(t) | Moment::Local(t) => t,
    }
}

/// A moment as it is in Sweden: a time with a zone is moved to Swedish time, one without is
/// taken as it is written.
fn swedish(m: Moment) -> chrono::NaiveDateTime {
    match m {
        Moment::At(utc) => utc + chrono::Duration::hours(crate::sun::swedish_offset(utc)),
        Moment::Local(t) => t,
    }
}

/// A moment read from an API: in UTC when its zone is known, or as it is written.
enum Moment {
    At(chrono::NaiveDateTime),
    Local(chrono::NaiveDateTime),
}

/// Reads ISO 8601 with or without a zone, a date alone, or a number of seconds since 1970
/// (milliseconds when it is that large, as ArcGIS gives them).
fn moment(v: &Value) -> Option<Moment> {
    let from_number = |x: f64| {
        let secs = if x.abs() > 1e11 { x / 1000.0 } else { x };
        chrono::DateTime::from_timestamp(secs.floor() as i64, ((secs - secs.floor()) * 1e9) as u32).map(|d| Moment::At(d.naive_utc()))
    };
    match v {
        Value::Null => None,
        Value::Int(i) => from_number(*i as f64),
        Value::Real(x) => from_number(*x),
        other => {
            let t = other.to_text();
            let t = t.trim();
            if let Ok(x) = t.parse::<f64>() {
                return from_number(x);
            }
            if let Ok(d) = chrono::DateTime::parse_from_rfc3339(t) {
                return Some(Moment::At(d.naive_utc()));
            }
            // 2026-10-17T08:00:00+0200, and with a space instead of the T.
            for f in ["%Y-%m-%dT%H:%M:%S%.f%z", "%Y-%m-%d %H:%M:%S%.f%z", "%Y-%m-%dT%H:%M%z"] {
                if let Ok(d) = chrono::DateTime::parse_from_str(t, f) {
                    return Some(Moment::At(d.naive_utc()));
                }
            }
            for f in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
                if let Ok(d) = chrono::NaiveDateTime::parse_from_str(t, f) {
                    return Some(Moment::Local(d));
                }
            }
            chrono::NaiveDate::parse_from_str(t, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)).map(Moment::Local)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, fields: &[&str], attrs: Vec<Value>) -> Result<Value> {
        let names: Vec<String> = fields.iter().map(|s| s.to_string()).collect();
        let f = Feature { z: None, attrs, geom: None };
        compile(src, &names)?.eval(&Row { feature: &f, index: 0, geographic: Some(false) })
    }
    fn t(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    #[test]
    fn arithmetic() {
        assert_eq!(run("1 + 2 * 3", &[], vec![]).unwrap(), Value::Int(7));
        assert_eq!(run("(1 + 2) * 3", &[], vec![]).unwrap(), Value::Int(9));
        assert_eq!(run("7 / 2", &[], vec![]).unwrap(), Value::Real(3.5));
        assert_eq!(run("7 // 2", &[], vec![]).unwrap(), Value::Int(3));
        assert_eq!(run("7 % 4", &[], vec![]).unwrap(), Value::Int(3));
        assert_eq!(run("-x + 1", &["x"], vec![Value::Int(5)]).unwrap(), Value::Int(-4));
        assert_eq!(run("1 / 0", &[], vec![]).unwrap(), Value::Null);
        assert_eq!(run("x + 1", &["x"], vec![Value::Null]).unwrap(), Value::Null);
        assert_eq!(run("x * 2", &["x"], vec![t("2,5")]).unwrap(), Value::Real(5.0));
        assert_eq!(run("round(population / 1000, 1)", &["population"], vec![Value::Int(28340)]).unwrap(), Value::Real(28.3));
        assert_eq!(run("round(2.5)", &[], vec![]).unwrap(), Value::Int(3));
        assert_eq!(run("max(1, 2.5, 2)", &[], vec![]).unwrap(), Value::Real(2.5));
        assert!(run("x + 1", &["x"], vec![t("abc")]).is_err());
    }

    #[test]
    fn comparisons_and_logic() {
        let f = ["population", "municipality"];
        let row = || vec![Value::Int(16000), t("Mark")];
        assert_eq!(run("population >= 10000 AND municipality <> 'Mark'", &f, row()).unwrap(), Value::Bool(false));
        assert_eq!(run("population >= 10000 or municipality = 'X'", &f, row()).unwrap(), Value::Bool(true));
        assert_eq!(run("not population < 10000", &f, row()).unwrap(), Value::Bool(true));
        assert_eq!(run("municipality IN ('Mark', 'Borås')", &f, row()).unwrap(), Value::Bool(true));
        assert_eq!(run("municipality NOT IN ('Borås')", &f, row()).unwrap(), Value::Bool(true));
        assert_eq!(run("municipality LIKE 'Ma%'", &f, row()).unwrap(), Value::Bool(true));
        assert_eq!(run("municipality LIKE 'ma_k'", &f, row()).unwrap(), Value::Bool(false));
        assert_eq!(run("municipality ILIKE 'ma_k'", &f, row()).unwrap(), Value::Bool(true));
        assert_eq!(run("municipality IS NOT NULL", &f, row()).unwrap(), Value::Bool(true));
        // A number against text that is a number compares as numbers, text against text as text.
        assert_eq!(run("x = 1480", &["x"], vec![t("1480")]).unwrap(), Value::Bool(true));
        assert_eq!(run("x < '9'", &["x"], vec![t("10")]).unwrap(), Value::Bool(true));
        // Null propagates, but AND/OR can still be decided.
        assert_eq!(run("x > 1", &["x"], vec![Value::Null]).unwrap(), Value::Null);
        assert_eq!(run("x > 1 OR true", &["x"], vec![Value::Null]).unwrap(), Value::Bool(true));
        assert_eq!(run("x > 1 AND false", &["x"], vec![Value::Null]).unwrap(), Value::Bool(false));
        assert_eq!(run("x IS NULL", &["x"], vec![Value::Null]).unwrap(), Value::Bool(true));
    }

    #[test]
    fn text_functions() {
        // The third field has a space and a non-ASCII letter in its name on purpose.
        let f = ["code", "name", "långt namn"];
        let row = || vec![Value::Int(114), t("Upplands Väsby"), t("x")];
        assert_eq!(run("lpad(to_string(code), 4, '0')", &f, row()).unwrap(), t("0114"));
        assert_eq!(run("upper(left(name, 3)) || '-' || code", &f, row()).unwrap(), t("UPP-114"));
        assert_eq!(run("\"långt namn\" || 'y'", &f, row()).unwrap(), t("xy"));
        assert_eq!(run("substr(name, 10, 3)", &f, row()).unwrap(), t("Väs"));
        assert_eq!(run("length(name)", &f, row()).unwrap(), Value::Int(14));
        assert_eq!(run("replace(name, 'Väsby', 'V')", &f, row()).unwrap(), t("Upplands V"));
        assert_eq!(run("coalesce(null, name)", &f, row()).unwrap(), t("Upplands Väsby"));
        assert_eq!(run("if(code < 1000, 'low', 'high')", &f, row()).unwrap(), t("low"));
        assert_eq!(run("concat(name, null, 1)", &f, row()).unwrap(), t("Upplands Väsby1"));
        assert_eq!(run("'it''s'", &f, row()).unwrap(), t("it's"));
        assert_eq!(run("right(name, 5)", &f, row()).unwrap(), t("Väsby"));
    }

    #[test]
    fn case_power_and_mathematics() {
        let f = ["h"];
        let case = "CASE WHEN h > 20 THEN 'high' WHEN h > 10 THEN 'middle' ELSE 'low' END";
        assert_eq!(run(case, &f, vec![Value::Int(25)]).unwrap(), t("high"));
        assert_eq!(run(case, &f, vec![Value::Int(15)]).unwrap(), t("middle"));
        assert_eq!(run(case, &f, vec![Value::Null]).unwrap(), t("low"));
        assert_eq!(run("case when h = 1 then 'one' end", &f, vec![Value::Int(2)]).unwrap(), Value::Null);
        assert_eq!(run("2 ^ 10", &[], vec![]).unwrap(), Value::Int(1024));
        assert_eq!(run("-2 ^ 2", &[], vec![]).unwrap(), Value::Int(-4));
        assert_eq!(run("2 ^ 3 ^ 2", &[], vec![]).unwrap(), Value::Int(512));
        assert_eq!(run("2 ^ -1", &[], vec![]).unwrap(), Value::Real(0.5));
        assert_eq!(run("power(9, 0.5)", &[], vec![]).unwrap(), Value::Real(3.0));
        assert_eq!(run("log10(1000)", &[], vec![]).unwrap(), Value::Real(3.0));
        assert_eq!(run("round(log(2, 8), 6)", &[], vec![]).unwrap(), Value::Real(3.0));
        assert_eq!(run("ln(0)", &[], vec![]).unwrap(), Value::Null);
        assert_eq!(run("round(sin(radians(30)), 6)", &[], vec![]).unwrap(), Value::Real(0.5));
        assert_eq!(run("round(degrees(atan2(1, 1)))", &[], vec![]).unwrap(), Value::Int(45));
        assert_eq!(run("clamp(0, 12, 10)", &[], vec![]).unwrap(), Value::Int(10));
        assert!(run("case h > 1 then 2 end", &f, vec![Value::Int(1)]).unwrap_err().0.contains("WHEN"));
        assert!(run("case when h > 1 then 2", &f, vec![Value::Int(1)]).unwrap_err().0.contains("END"));
    }

    #[test]
    fn text_search_and_regular_expressions() {
        let f = ["a"];
        let row = || vec![t("Storgatan 12, 411 38 Göteborg")];
        assert_eq!(run("regexp_substr(a, '(\\d{3} ?\\d{2})')", &f, row()).unwrap(), t("411 38"));
        assert_eq!(run("regexp_replace(a, '(\\d+) (\\d+)', '\\1\\2')", &f, row()).unwrap(), t("Storgatan 12, 41138 Göteborg"));
        // A $ in the replacement is a $, not a group.
        assert_eq!(run("regexp_replace('a-b', '-', '$$')", &[], vec![]).unwrap(), t("a$$b"));
        assert_eq!(run("regexp_match(a, 'Göte')", &f, row()).unwrap(), Value::Int(22));
        assert_eq!(run("regexp_match(a, 'Malmö')", &f, row()).unwrap(), Value::Int(0));
        assert_eq!(run("strpos(a, 'Göteborg')", &f, row()).unwrap(), Value::Int(22));
        assert_eq!(run("starts_with(a, 'Stor')", &f, row()).unwrap(), Value::Bool(true));
        assert_eq!(run("nullif('-', '-')", &[], vec![]).unwrap(), Value::Null);
        assert!(run("regexp_match(a, '(')", &f, row()).unwrap_err().0.contains("pattern"));
    }

    #[test]
    fn variables_of_the_geometry() {
        use geo::{point, polygon};
        let names: Vec<String> = vec![];
        let ev = |src: &str, g: geo::Geometry<f64>, z: Option<Vec<f64>>| {
            let f = Feature { z: z.map(|v| v.into()), attrs: vec![], geom: Some(g) };
            compile(src, &names).unwrap().eval(&Row { feature: &f, index: 4, geographic: Some(false) }).unwrap()
        };
        let p: geo::Geometry<f64> = point!(x: 319000.0, y: 6400000.0).into();
        assert_eq!(ev("$x", p.clone(), None), Value::Real(319000.0));
        assert_eq!(ev("$y", p.clone(), None), Value::Real(6400000.0));
        assert_eq!(ev("$z", p.clone(), Some(vec![42.5])), Value::Real(42.5));
        assert_eq!(ev("$id", p.clone(), None), Value::Int(5));
        let sq: geo::Geometry<f64> = polygon![(x: 0.0, y: 0.0), (x: 10.0, y: 0.0), (x: 10.0, y: 5.0), (x: 0.0, y: 5.0)].into();
        assert_eq!(ev("$perimeter", sq.clone(), None), Value::Real(30.0));
        assert_eq!(ev("$x", sq.clone(), None), Value::Null);
        assert_eq!(ev("$xmax - $xmin", sq.clone(), None), Value::Real(10.0));
        assert_eq!(ev("$num_points", sq, None), Value::Int(5));
        assert!(compile("$foo", &names).unwrap_err().0.contains("$perimeter"));
    }

    #[test]
    fn errors_are_readable() {
        let e = run("popul > 1", &["population", "name"], vec![Value::Int(1), t("a")]).unwrap_err();
        assert!(e.0.contains("popul") && e.0.contains("population, name"), "{}", e.0);
        assert!(run("1 +", &[], vec![]).is_err());
        assert!(run("(1 + 2", &[], vec![]).unwrap_err().0.contains("closing parenthesis"));
        assert!(run("foo(1)", &[], vec![]).unwrap_err().0.contains("Unknown function"));
        assert!(run("round()", &[], vec![]).unwrap_err().0.contains("wrong number"));
        assert!(run("1 2", &[], vec![]).is_err());
        assert!(run("'abc", &[], vec![]).is_err());
        assert!(run("", &[], vec![]).is_err());
    }
}

#[cfg(test)]
mod case_tests {
    #[test]
    fn words_get_capitals() {
        assert_eq!(super::initcap("LILLA EDET"), "Lilla Edet");
        assert_eq!(super::initcap("dals-ed"), "Dals-Ed");
        assert_eq!(super::initcap("öckerö kommun"), "Öckerö Kommun");
        assert_eq!(super::initcap("o'neill"), "O'neill");
        assert_eq!(crate::nodes::change_case("ÅMÅL STAD", "first"), "Åmål stad");
        assert_eq!(crate::nodes::change_case("Åmål", "upper"), "ÅMÅL");
        assert_eq!(crate::nodes::change_case("Åmål", "lower"), "åmål");
    }
}

#[cfg(test)]
mod time_tests {
    use super::*;

    fn run(src: &str) -> Value {
        let c = compile(src, &[]).unwrap();
        let f = crate::model::Feature { attrs: vec![], geom: None, z: None };
        c.eval(&Row { feature: &f, index: 0, geographic: Some(false) }).unwrap()
    }

    #[test]
    fn times_from_apis_in_swedish_time() {
        // Summer: two hours ahead of UTC; winter: one.
        assert_eq!(run("format_time('2026-06-01T10:00:00Z')"), Value::Text("2026-06-01 12:00".into()));
        assert_eq!(run("format_time('2026-12-01T10:00:00Z')"), Value::Text("2026-12-01 11:00".into()));
        assert_eq!(run("format_time('2026-10-17T08:30:00+02:00', '%H:%M', 'utc')"), Value::Text("06:30".into()));
        // Milliseconds since 1970, as ArcGIS: 2026-01-01 00:00 UTC.
        assert_eq!(run("format_time(1767225600000, '%Y-%m-%d %H:%M')"), Value::Text("2026-01-01 01:00".into()));
        assert_eq!(run("format_time(1767225600, '%Y-%m-%d', 'utc')"), Value::Text("2026-01-01".into()));
        // Without a zone, as it is written.
        assert_eq!(run("format_time('2026-10-17T08:00:00', '%H:%M')"), Value::Text("08:00".into()));
        assert_eq!(run("(epoch('2026-10-17T09:30:00Z') - epoch('2026-10-17T09:00:00Z')) / 60"), Value::Real(30.0));
        assert_eq!(run("format_time('not a time')"), Value::Null);
        // Parts of a time, in Swedish time when the zone is given.
        assert_eq!(run("hour('2026-06-01T22:30:00Z')"), Value::Int(0));
        assert_eq!(run("day('2026-06-01T22:30:00Z')"), Value::Int(2));
        assert_eq!(run("year('2026-10-21')"), Value::Int(2026));
        assert_eq!(run("day_of_week('2026-10-21')"), Value::Int(3));
        assert_eq!(run("week('2026-10-21')"), Value::Int(43));
        assert_eq!(run("date_add('2026-10-21T08:00', 15, 'minutes')"), Value::Text("2026-10-21T08:15:00".into()));
        assert_eq!(run("date_add('2026-10-21T23:00:00Z', 2, 'hours')"), Value::Text("2026-10-22T01:00:00Z".into()));
        assert_eq!(run("date_diff('2026-10-21T09:30', '2026-10-21T08:00', 'minutes')"), Value::Real(90.0));
        assert_eq!(run("date_diff('2026-10-31', '2026-10-21')"), Value::Real(10.0));
    }
}
