//! A small CSS value resolver: the `:root` variables of `root.css`, the
//! ones a theme writes on `<html>`, and a theme's own `:root` overrides,
//! resolved into concrete colours, lengths and numbers.
//!
//! Supported: `var()` chains with fallbacks and cycle detection, the colour
//! notations of [`crate::color`], lengths in `px`, `rem` (16px), `em`, `%`,
//! `vw` and `vh`, times in `ms` and `s`, and `calc()` with `+ - * /`.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::color::{parse_color_tokens, Rgba};
use crate::js;
use crate::themes::ThemeApplication;

const APP_CSS: &str = include_str!("root.css");

/// Root font size: nothing in the app changes the browser default.
pub const ROOT_FONT_SIZE_PX: f64 = 16.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Token {
    Ident(String),
    /// Name lowercased, arguments as written.
    Function(String, Vec<Token>),
    Number(f64),
    Percentage(f64),
    /// Unit lowercased.
    Dimension(f64, String),
    Hash(String),
    Str(String),
    /// A parenthesized block that is not a function call.
    Block(Vec<Token>),
    Comma,
    Slash,
    Delim(char),
    Whitespace,
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || !c.is_ascii()
}

struct Tokenizer {
    chars: Vec<char>,
    pos: usize,
}

impl Tokenizer {
    fn peek(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn starts_ident(&self) -> bool {
        match (self.peek(0), self.peek(1)) {
            (Some('-'), Some(next)) => is_name_start(next) || next == '-',
            (Some(c), _) => is_name_start(c),
            _ => false,
        }
    }

    fn starts_number(&self) -> bool {
        let digit = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit());
        match self.peek(0) {
            Some(c) if c.is_ascii_digit() => true,
            Some('.') => digit(self.peek(1)),
            Some('+' | '-') => {
                digit(self.peek(1)) || (self.peek(1) == Some('.') && digit(self.peek(2)))
            }
            _ => false,
        }
    }

    fn name(&mut self) -> String {
        let start = self.pos;
        while self.peek(0).is_some_and(is_name_char) {
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }

    fn number(&mut self) -> f64 {
        let start = self.pos;
        if matches!(self.peek(0), Some('+' | '-')) {
            self.pos += 1;
        }
        while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek(0) == Some('.') && self.peek(1).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
            while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(0), Some('e' | 'E')) {
            let sign = usize::from(matches!(self.peek(1), Some('+' | '-')));
            if self.peek(1 + sign).is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1 + sign;
                while self.peek(0).is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse().unwrap_or(0.0)
    }

    /// Tokens until `)` (when `nested`) or the end.
    fn list(&mut self, nested: bool) -> Option<Vec<Token>> {
        let mut tokens = Vec::new();
        while let Some(c) = self.peek(0) {
            if c.is_whitespace() {
                while self.peek(0).is_some_and(char::is_whitespace) {
                    self.pos += 1;
                }
                tokens.push(Token::Whitespace);
                continue;
            }
            if c == ')' {
                if !nested {
                    return None;
                }
                self.pos += 1;
                return Some(tokens);
            }
            if self.starts_number() {
                let value = self.number();
                if self.peek(0) == Some('%') {
                    self.pos += 1;
                    tokens.push(Token::Percentage(value));
                } else if self.starts_ident() {
                    let unit = self.name().to_ascii_lowercase();
                    tokens.push(Token::Dimension(value, unit));
                } else {
                    tokens.push(Token::Number(value));
                }
                continue;
            }
            if self.starts_ident() {
                let name = self.name();
                if self.peek(0) == Some('(') {
                    self.pos += 1;
                    let args = self.list(true)?;
                    tokens.push(Token::Function(name.to_ascii_lowercase(), args));
                } else {
                    tokens.push(Token::Ident(name));
                }
                continue;
            }
            self.pos += 1;
            match c {
                '#' => tokens.push(Token::Hash(self.name())),
                '(' => tokens.push(Token::Block(self.list(true)?)),
                ',' => tokens.push(Token::Comma),
                '/' => tokens.push(Token::Slash),
                '"' | '\'' => {
                    let start = self.pos;
                    while self.peek(0).is_some_and(|next| next != c) {
                        self.pos += 1;
                    }
                    let text = self.chars[start..self.pos].iter().collect();
                    self.pos += 1;
                    tokens.push(Token::Str(text));
                }
                other => tokens.push(Token::Delim(other)),
            }
        }
        (!nested).then_some(tokens)
    }
}

/// Tokenizes a value; `None` when its parentheses do not balance.
pub(crate) fn tokenize(text: &str) -> Option<Vec<Token>> {
    Tokenizer {
        chars: text.chars().collect(),
        pos: 0,
    }
    .list(false)
}

pub(crate) fn trim_whitespace(tokens: &[Token]) -> &[Token] {
    let start = tokens
        .iter()
        .position(|token| *token != Token::Whitespace)
        .unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|token| *token != Token::Whitespace)
        .map_or(start, |index| index + 1);
    &tokens[start..end]
}

/// The comma-separated parts of a list, at its top level.
pub(crate) fn split_commas(tokens: &[Token]) -> Vec<&[Token]> {
    tokens.split(|token| *token == Token::Comma).collect()
}

/// The tokens of a list, whitespace removed.
pub(crate) fn components(tokens: &[Token]) -> Vec<&Token> {
    tokens
        .iter()
        .filter(|token| **token != Token::Whitespace)
        .collect()
}

/// A length as CSS keeps it before layout: a sum of px, em, %, vw and vh,
/// which is what `calc(100% - 26px)` is. `rem` is folded into px.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Length {
    pub px: f32,
    pub em: f32,
    pub percent: f32,
    pub vw: f32,
    pub vh: f32,
}

impl Length {
    pub const fn px(px: f32) -> Length {
        Length {
            px,
            em: 0.0,
            percent: 0.0,
            vw: 0.0,
            vh: 0.0,
        }
    }

    /// The length in px when it does not depend on layout.
    pub fn as_px(self) -> Option<f32> {
        (self.em == 0.0 && self.percent == 0.0 && self.vw == 0.0 && self.vh == 0.0)
            .then_some(self.px)
    }

    /// The length in px against a font size, a percentage basis and a
    /// viewport (all in px).
    pub fn resolve(self, font_px: f32, percent_basis: f32, viewport: (f32, f32)) -> f32 {
        self.px
            + self.em * font_px
            + self.percent / 100.0 * percent_basis
            + self.vw / 100.0 * viewport.0
            + self.vh / 100.0 * viewport.1
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Number,
    Length,
    Time,
}

/// A `calc()` operand or result.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Quantity {
    kind: Kind,
    /// Number value, px, or ms, by kind.
    base: f64,
    em: f64,
    percent: f64,
    vw: f64,
    vh: f64,
}

impl Quantity {
    fn new(kind: Kind, base: f64) -> Quantity {
        Quantity {
            kind,
            base,
            em: 0.0,
            percent: 0.0,
            vw: 0.0,
            vh: 0.0,
        }
    }

    fn scale(self, factor: f64) -> Quantity {
        Quantity {
            kind: self.kind,
            base: self.base * factor,
            em: self.em * factor,
            percent: self.percent * factor,
            vw: self.vw * factor,
            vh: self.vh * factor,
        }
    }

    fn add(self, other: Quantity, sign: f64) -> Option<Quantity> {
        (self.kind == other.kind).then_some(Quantity {
            kind: self.kind,
            base: self.base + sign * other.base,
            em: self.em + sign * other.em,
            percent: self.percent + sign * other.percent,
            vw: self.vw + sign * other.vw,
            vh: self.vh + sign * other.vh,
        })
    }

    fn from_token(token: &Token) -> Option<Quantity> {
        let length = |field: fn(&mut Quantity) -> &mut f64, value: f64| {
            let mut quantity = Quantity::new(Kind::Length, 0.0);
            *field(&mut quantity) = value;
            quantity
        };
        match token {
            Token::Number(value) => Some(Quantity::new(Kind::Number, *value)),
            Token::Percentage(value) => Some(length(|q| &mut q.percent, *value)),
            Token::Dimension(value, unit) => match unit.as_str() {
                "px" => Some(Quantity::new(Kind::Length, *value)),
                "rem" => Some(Quantity::new(Kind::Length, value * ROOT_FONT_SIZE_PX)),
                "em" => Some(length(|q| &mut q.em, *value)),
                "vw" => Some(length(|q| &mut q.vw, *value)),
                "vh" => Some(length(|q| &mut q.vh, *value)),
                "ms" => Some(Quantity::new(Kind::Time, *value)),
                "s" => Some(Quantity::new(Kind::Time, value * 1000.0)),
                _ => None,
            },
            Token::Function(name, args) if name == "calc" => calc(args),
            Token::Block(inner) => calc(inner),
            _ => None,
        }
    }
}

/// `calc()` contents: sums of products, standard precedence.
fn calc(tokens: &[Token]) -> Option<Quantity> {
    let items = components(tokens);
    let mut cursor = 0;
    let result = calc_sum(&items, &mut cursor)?;
    (cursor == items.len()).then_some(result)
}

fn calc_sum(items: &[&Token], cursor: &mut usize) -> Option<Quantity> {
    let mut total = calc_product(items, cursor)?;
    while let Some(Token::Delim(op @ ('+' | '-'))) = items.get(*cursor) {
        *cursor += 1;
        let sign = if *op == '+' { 1.0 } else { -1.0 };
        let next = calc_product(items, cursor)?;
        total = total.add(next, sign)?;
    }
    Some(total)
}

fn calc_product(items: &[&Token], cursor: &mut usize) -> Option<Quantity> {
    let mut total = Quantity::from_token(items.get(*cursor)?)?;
    *cursor += 1;
    loop {
        match items.get(*cursor) {
            Some(Token::Delim('*')) => {
                *cursor += 1;
                let next = Quantity::from_token(items.get(*cursor)?)?;
                *cursor += 1;
                total = match (total.kind, next.kind) {
                    (Kind::Number, _) => next.scale(total.base),
                    (_, Kind::Number) => total.scale(next.base),
                    _ => return None,
                };
            }
            Some(Token::Slash) => {
                *cursor += 1;
                let next = Quantity::from_token(items.get(*cursor)?)?;
                *cursor += 1;
                if next.kind != Kind::Number || next.base == 0.0 {
                    return None;
                }
                total = total.scale(1.0 / next.base);
            }
            _ => return Some(total),
        }
    }
}

fn single_quantity(tokens: &[Token]) -> Option<Quantity> {
    match components(tokens)[..] {
        [token] => Quantity::from_token(token),
        _ => None,
    }
}

fn parse_length_tokens(tokens: &[Token]) -> Option<Length> {
    let quantity = single_quantity(tokens)?;
    match quantity.kind {
        Kind::Length => Some(Length {
            px: quantity.base as f32,
            em: quantity.em as f32,
            percent: quantity.percent as f32,
            vw: quantity.vw as f32,
            vh: quantity.vh as f32,
        }),
        // A unitless zero is a length.
        Kind::Number if quantity.base == 0.0 => Some(Length::default()),
        _ => None,
    }
}

/// One `box-shadow` layer, lengths in px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub inset: bool,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    /// `None` when the layer names no colour (it would be `currentcolor`).
    pub color: Option<Rgba>,
}

fn parse_shadow_tokens(tokens: &[Token]) -> Option<Vec<Shadow>> {
    if let [Token::Ident(name)] = components(tokens)[..] {
        if name.eq_ignore_ascii_case("none") {
            return Some(Vec::new());
        }
    }
    split_commas(tokens)
        .into_iter()
        .map(|layer| {
            let mut inset = false;
            let mut lengths = Vec::new();
            let mut color = None;
            for token in components(layer) {
                if let Token::Ident(name) = token {
                    if name.eq_ignore_ascii_case("inset") {
                        inset = true;
                        continue;
                    }
                }
                if let Some(length) = parse_length_tokens(std::slice::from_ref(token)) {
                    lengths.push(length.as_px()?);
                } else {
                    color = Some(parse_color_tokens(std::slice::from_ref(token))?);
                }
            }
            if !(2..=4).contains(&lengths.len()) {
                return None;
            }
            Some(Shadow {
                inset,
                offset_x: lengths[0],
                offset_y: lengths[1],
                blur: lengths.get(2).copied().unwrap_or(0.0),
                spread: lengths.get(3).copied().unwrap_or(0.0),
                color,
            })
        })
        .collect()
}

/// Something in a theme's custom CSS the resolver does not apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsupportedCss {
    /// A rule whose selector is not `:root`.
    Rule(String),
    /// A declaration in `:root` that is not a custom property.
    Declaration(String),
    /// An at-rule (`@media`, `@keyframes`...).
    AtRule(String),
    /// Text the parser could not make sense of.
    Malformed(String),
}

impl std::fmt::Display for UnsupportedCss {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnsupportedCss::Rule(selector) => write!(f, "rule `{selector}`"),
            UnsupportedCss::Declaration(property) => write!(f, "declaration `{property}` in :root"),
            UnsupportedCss::AtRule(name) => write!(f, "at-rule `{name}`"),
            UnsupportedCss::Malformed(text) => write!(f, "malformed CSS `{text}`"),
        }
    }
}

/// One custom property declaration from a stylesheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomProperty {
    pub name: String,
    pub value: String,
    pub important: bool,
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(length) => rest = &rest[start + 2 + length + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Index of the first `stop` char at depth 0 outside strings, from `from`.
fn find_top_level(text: &str, from: usize, stops: &[char]) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote = None;
    let mut escaped = false;
    for (offset, c) in text[from..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
            continue;
        }
        if depth == 0 && stops.contains(&c) {
            return Some(from + offset);
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Splits `name: value; ...` and keeps the custom properties.
fn parse_declarations(block: &str, unsupported: &mut Vec<UnsupportedCss>) -> Vec<CustomProperty> {
    let mut properties = Vec::new();
    let mut start = 0;
    while start < block.len() {
        let end = find_top_level(block, start, &[';']).unwrap_or(block.len());
        let declaration = block[start..end].trim();
        start = end + 1;
        if declaration.is_empty() {
            continue;
        }
        let Some((name, value)) = declaration.split_once(':') else {
            unsupported.push(UnsupportedCss::Malformed(declaration.to_owned()));
            continue;
        };
        let name = name.trim();
        if !name.starts_with("--") {
            unsupported.push(UnsupportedCss::Declaration(name.to_owned()));
            continue;
        }
        let mut value = value.trim();
        let mut important = false;
        if let Some(bang) = value.rfind('!') {
            if value[bang + 1..].trim().eq_ignore_ascii_case("important") {
                important = true;
                value = value[..bang].trim_end();
            }
        }
        properties.push(CustomProperty {
            name: name.to_owned(),
            value: value.to_owned(),
            important,
        });
    }
    properties
}

/// Reads the custom properties of every `:root` rule of a stylesheet, and
/// lists everything else it holds as unsupported.
pub fn parse_root_custom_properties(css: &str) -> (Vec<CustomProperty>, Vec<UnsupportedCss>) {
    let css = strip_comments(css);
    let mut properties = Vec::new();
    let mut unsupported = Vec::new();
    let mut start = 0;
    while start < css.len() {
        let Some(end) = find_top_level(&css, start, &['{', ';', '}']) else {
            let rest = css[start..].trim();
            if !rest.is_empty() {
                unsupported.push(UnsupportedCss::Malformed(rest.to_owned()));
            }
            break;
        };
        let prelude = css[start..end].trim();
        let terminator = css[end..].chars().next();
        if terminator != Some('{') {
            if prelude.starts_with('@') {
                unsupported.push(UnsupportedCss::AtRule(at_rule_name(prelude)));
            } else if !prelude.is_empty() || terminator == Some('}') {
                unsupported.push(UnsupportedCss::Malformed(
                    css[start..=end].trim().to_owned(),
                ));
            }
            start = end + 1;
            continue;
        }
        let Some(close) = find_top_level(&css, end + 1, &['}']) else {
            unsupported.push(UnsupportedCss::Malformed(css[start..].trim().to_owned()));
            break;
        };
        let block = &css[end + 1..close];
        if prelude.starts_with('@') {
            unsupported.push(UnsupportedCss::AtRule(at_rule_name(prelude)));
        } else if prelude == ":root" {
            properties.extend(parse_declarations(block, &mut unsupported));
        } else {
            unsupported.push(UnsupportedCss::Rule(prelude.to_owned()));
        }
        start = close + 1;
    }
    (properties, unsupported)
}

fn at_rule_name(prelude: &str) -> String {
    prelude
        .split(|c: char| c.is_whitespace() || c == '(' || c == '"' || c == '\'')
        .next()
        .unwrap_or(prelude)
        .to_owned()
}

/// The `:root` custom properties of `root.css`, parsed once.
pub fn app_root_properties() -> &'static [CustomProperty] {
    static ROOT: OnceLock<Vec<CustomProperty>> = OnceLock::new();
    ROOT.get_or_init(|| parse_root_custom_properties(APP_CSS).0)
}

/// The custom properties in effect on `<html>`, ready to resolve.
///
/// Cascade, lowest first: the `:root` block of `root.css`, the theme's own CSS,
/// the variables a theme application writes on the inline style, then the
/// theme's own `!important` declarations (an inline style loses to those).
#[derive(Clone, Debug, Default)]
pub struct ResolvedTheme {
    declared: HashMap<String, String>,
}

impl ResolvedTheme {
    /// Only `root.css`: what the app paints with before a theme lands.
    pub fn root() -> ResolvedTheme {
        let mut resolved = ResolvedTheme::default();
        for property in app_root_properties() {
            resolved
                .declared
                .insert(property.name.clone(), property.value.clone());
        }
        resolved
    }

    /// `root.css`, then everything one theme application writes.
    pub fn new(application: &ThemeApplication) -> ResolvedTheme {
        let mut resolved = ResolvedTheme::root();
        let theme_properties = match &application.css {
            Some(css) => parse_root_custom_properties(css).0,
            None => Vec::new(),
        };
        for property in theme_properties
            .iter()
            .filter(|property| !property.important)
        {
            resolved
                .declared
                .insert(property.name.clone(), property.value.clone());
        }
        for (name, value) in &application.vars {
            resolved.declared.insert((*name).to_owned(), value.clone());
        }
        for property in theme_properties
            .iter()
            .filter(|property| property.important)
        {
            resolved
                .declared
                .insert(property.name.clone(), property.value.clone());
        }
        resolved
    }

    /// The declared value, as written.
    pub fn raw(&self, name: &str) -> Option<&str> {
        self.declared.get(name).map(String::as_str)
    }

    /// The value with every `var()` replaced: the computed value of a custom
    /// property. `None` when it is undefined, cyclic, or uses an undefined
    /// variable with no fallback.
    pub fn substitute(&self, name: &str) -> Option<String> {
        self.substitute_property(name, &mut Vec::new())
    }

    fn substitute_property(&self, name: &str, stack: &mut Vec<String>) -> Option<String> {
        if stack.iter().any(|entry| entry == name) {
            return None;
        }
        let value = self.declared.get(name)?;
        stack.push(name.to_owned());
        let result = self.substitute_text(value, stack);
        stack.pop();
        result
    }

    fn substitute_text(&self, text: &str, stack: &mut Vec<String>) -> Option<String> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = find_var(rest) {
            out.push_str(&rest[..start]);
            let args_start = start + 4;
            let close = find_top_level(rest, args_start, &[')'])?;
            let args = &rest[args_start..close];
            let (name, fallback) = match find_top_level(args, 0, &[',']) {
                Some(comma) => (args[..comma].trim(), Some(&args[comma + 1..])),
                None => (args.trim(), None),
            };
            let value = match self.substitute_property(name, stack) {
                Some(value) => value,
                None => self.substitute_text(js::trim(fallback?), stack)?,
            };
            out.push_str(&value);
            rest = &rest[close + 1..];
        }
        out.push_str(rest);
        Some(out.trim().to_owned())
    }

    fn tokens(&self, name: &str) -> Option<Vec<Token>> {
        tokenize(&self.substitute(name)?)
    }

    fn expression_tokens(&self, expression: &str) -> Option<Vec<Token>> {
        tokenize(&self.substitute_text(expression, &mut Vec::new())?)
    }

    pub fn color(&self, name: &str) -> Option<Rgba> {
        parse_color_tokens(&self.tokens(name)?)
    }

    pub fn length(&self, name: &str) -> Option<Length> {
        parse_length_tokens(&self.tokens(name)?)
    }

    /// A length that does not depend on layout, in px.
    pub fn length_px(&self, name: &str) -> Option<f32> {
        self.length(name)?.as_px()
    }

    pub fn number(&self, name: &str) -> Option<f32> {
        number_from(&self.tokens(name)?)
    }

    /// A time (`260ms`, `calc(260ms * var(--motion-scale))`) in ms.
    pub fn duration_ms(&self, name: &str) -> Option<f32> {
        duration_from(&self.tokens(name)?)
    }

    pub fn shadow(&self, name: &str) -> Option<Vec<Shadow>> {
        parse_shadow_tokens(&self.tokens(name)?)
    }

    /// A colour written in a component's CSS, such as
    /// `color-mix(in srgb, var(--fg) 65%, transparent)`.
    pub fn eval_color(&self, expression: &str) -> Option<Rgba> {
        parse_color_tokens(&self.expression_tokens(expression)?)
    }

    /// Every declared name, in no particular order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.declared.keys().map(String::as_str)
    }
}

fn number_from(tokens: &[Token]) -> Option<f32> {
    let quantity = single_quantity(tokens)?;
    (quantity.kind == Kind::Number).then_some(quantity.base as f32)
}

fn duration_from(tokens: &[Token]) -> Option<f32> {
    let quantity = single_quantity(tokens)?;
    (quantity.kind == Kind::Time).then_some(quantity.base as f32)
}

/// Byte offset of the next `var(` that starts a function name.
fn find_var(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    (0..text.len()).find(|&index| {
        bytes[index..].len() >= 4
            && bytes[index..index + 4].eq_ignore_ascii_case(b"var(")
            && (index == 0
                || !(bytes[index - 1].is_ascii_alphanumeric()
                    || matches!(bytes[index - 1], b'-' | b'_')))
    })
}

#[cfg(test)]
mod tests;
