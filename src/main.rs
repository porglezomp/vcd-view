#![feature(nll)]

extern crate vcd;

use std::collections::BTreeMap;
use vcd::{Command, IdCode, Parser, ScopeItem, Var};

static HEADER: &str = include_str!("header.html");
static FOOTER: &str = include_str!("footer.html");

#[derive(Debug, PartialEq)]
enum Value {
    Scalar(vcd::Value),
    Vector(Vec<vcd::Value>),
}

#[derive(Debug)]
struct Wave {
    var: Var,
    values: Vec<(u64, Value)>,
    svg: Option<Svg>,
}

#[derive(Debug)]
struct Svg {
    wave: String,
    bits: Vec<String>,
}

fn main() -> std::io::Result<()> {
    let mut parser = Parser::new(std::io::stdin());
    let header = parser.parse_header()?;
    let mut waves: BTreeMap<_, _> = make_waves(&header.items);
    let mut time = 0;
    for command in parser {
        match command {
            Ok(Command::Timestamp(t)) => time = t,
            Ok(Command::ChangeVector(id, v)) => waves
                .get_mut(&id)
                .unwrap()
                .values
                .push((time, Value::Vector(v))),
            Ok(Command::ChangeScalar(id, v)) => waves
                .get_mut(&id)
                .unwrap()
                .values
                .push((time, Value::Scalar(v))),
            _ => (),
        }
    }
    for wave in waves.values_mut() {
        for &mut (time, ref mut item) in &mut wave.values {
            normalize(item);
            if size(item) != wave.var.size as usize {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Value at time {} in {} has invalid width (expected {}, got {})",
                        time,
                        wave.var.reference,
                        wave.var.size,
                        size(item),
                    ),
                ));
            }
        }
    }
    let end_time = waves
        .iter()
        .map(|(_, w)| w.values.last().map(|&(t, _)| t).unwrap_or(0))
        .max()
        .unwrap_or(0)
        + 10;
    for wave in waves.values_mut() {
        render_svg(wave, end_time);
    }

    println!("{}", HEADER);
    print_vars(&header);
    println!(r#"<div id="display">"#);
    print_names(&header);
    print_waves(&header, &waves);
    println!("{}", FOOTER);

    Ok(())
}

fn walk_dfs(
    open: impl Fn(),
    open_scope: impl Fn(&vcd::Scope),
    do_var: impl Fn(&Var),
    close_scope: impl Fn(&vcd::Scope),
    close: impl Fn(),
    header: &vcd::Header,
) {
    fn walk_scope(
        open_scope: &impl Fn(&vcd::Scope),
        do_var: &impl Fn(&Var),
        close_scope: &impl Fn(&vcd::Scope),
        s: &vcd::Scope,
    ) {
        open_scope(s);
        for child in &s.children {
            match child {
                ScopeItem::Var(var) => do_var(var),
                ScopeItem::Scope(s) => walk_scope(open_scope, do_var, close_scope, s),
            }
        }
        close_scope(s);
    }
    open();
    for item in &header.items {
        match item {
            ScopeItem::Var(var) => do_var(var),
            ScopeItem::Scope(scope) => walk_scope(&open_scope, &do_var, &close_scope, scope),
        }
    }
    close();
}

fn print_vars(header: &vcd::Header) {
    walk_dfs(
        || println!("<ul>"),
        |s| {
            println!(
                r#"<li class="scope closed">
<div class="arrow"></div><label><input class="scope-checkbox" type="checkbox" data-name="{name}" checked/>{name}</label>
<ul>"#,
                name = s.identifier,
            )
        },
        |v| {
            println!(
                r#"<li class="var">
<label><input type="checkbox" data-id="{id}" checked/>{name}</label></li>"#,
                id = v.code,
                name = v.reference,
            )
        },
        |_| println!("</ul>\n</li>"),
        || println!("</ul></div>"),
        header,
    );
}

fn print_names(header: &vcd::Header) {
    walk_dfs(
        || println!(r#"<div id="labels"><ul>"#),
        |_| (),
        |v| {
            println!(
                r#"<li data-id="{id}">{name}</li>"#,
                id = v.code,
                name = v.reference
            )
        },
        |_| (),
        || println!("</ul></div>"),
        header,
    );
}

fn print_waves(header: &vcd::Header, waves: &BTreeMap<IdCode, Wave>) {
    let wave_count: usize = waves
        .values()
        .map(|wave| match wave.svg {
            Some(Svg { ref bits, .. }) => 1 + bits.len(),
            _ => 0,
        }).sum();
    let height = wave_count * 40 + 20;
    walk_dfs(
        || println!(r#"<div id="waves" style="height: {}px;"><ul>"#, height),
        |_| (),
        |v| {
            if let Some(ref svg) = waves[&v.code].svg {
                println!(
                    r#"<li data-id="{id}">{wave}</li>"#,
                    id = v.code,
                    wave = svg.wave
                );
            }
        },
        |_| (),
        || println!("</ul></div>"),
        header,
    );
}

fn render_svg(wave: &mut Wave, end_time: u64) {
    if wave.values.is_empty() {
        return;
    }

    enum State<'a> {
        Wave(Vec<(u64, bool)>),
        Vec(u64, &'a [vcd::Value]),
        X(u64),
    }
    let mut svg_parts = Vec::new();

    fn add_undet(parts: &mut Vec<String>, start: u64, end: u64) {
        if start == end {
            return;
        }
        parts.push(format!(
            r#"<rect class="x" rx="1" ry="1" x="{}" y="0" width="{}" height="10"/>"#,
            start,
            end - start
        ));
    }

    fn add_vec(parts: &mut Vec<String>, start: u64, end: u64, value: &[vcd::Value]) {
        parts.push(format!(
            r#"<rect class="vec" rx="1" ry="1" x="{x}" y="0" width="{width}" height="10"/>
<svg x="{x}" y="0" width="{width}" height="10" preserveAspectRatio="none" viewBox="0 0 {width} 10">
<text x="{center}" y="8">{text}</text>
</svg>"#,
            x = start,
            width = end - start,
            center = (end - start) / 2,
            text = FmtVec(value),
        ));
    }

    fn add_wave(parts: &mut Vec<String>, items: Vec<(u64, bool)>, end_time: u64) {
        fn wave(b: bool) -> u32 {
            if b {
                0
            } else {
                10
            }
        }
        assert!(!items.is_empty());
        let (first, items) = items.split_first().unwrap();
        let mut prev = first;
        let mut text = format!(r#"<polyline points="{} {},"#, first.0, wave(first.1));
        let mut y = 0;
        for item in items {
            y = wave(item.1);
            text += &format!("{} {},{} {},", item.0, wave(prev.1), item.0, y);
            prev = item;
        }
        text += &format!(r#"{} {}"/>"#, end_time, y);
        parts.push(text)
    }

    let mut state = State::X(0);
    if wave.var.size == 1 {
        for &(time, ref point) in &wave.values {
            use vcd::Value::*;
            state = match (state, point) {
                (State::X(x), Value::Scalar(V0)) => {
                    add_undet(&mut svg_parts, x, time);
                    State::Wave(vec![(time, false)])
                }
                (State::X(x), Value::Scalar(V1)) => {
                    add_undet(&mut svg_parts, x, time);
                    State::Wave(vec![(time, true)])
                }
                (State::Wave(mut items), Value::Scalar(V0)) => {
                    items.push((time, false));
                    State::Wave(items)
                }
                (State::Wave(mut items), Value::Scalar(V1)) => {
                    items.push((time, true));
                    State::Wave(items)
                }
                (State::Wave(items), Value::Scalar(X)) | (State::Wave(items), Value::Scalar(Z)) => {
                    add_wave(&mut svg_parts, items, time);
                    State::X(time)
                }
                (state, _) => state,
            }
        }
    } else {
        for &(time, ref point) in &wave.values {
            use vcd::Value::*;
            state = match (state, point) {
                (State::X(x), Value::Vector(items)) => {
                    add_undet(&mut svg_parts, x, time);
                    State::Vec(time, &items)
                }
                (State::Vec(x, items), Value::Vector(items2)) => {
                    if items != &items2[..] {
                        add_vec(&mut svg_parts, x, time, items);
                        State::Vec(time, &items2)
                    } else {
                        State::Vec(x, &items)
                    }
                }
                (State::Vec(x, items), Value::Scalar(X))
                | (State::Vec(x, items), Value::Scalar(Z)) => {
                    add_vec(&mut svg_parts, x, time, items);
                    State::X(time)
                }
                (state, _) => state,
            }
        }
    }
    match state {
        State::X(x) => add_undet(&mut svg_parts, x, end_time),
        State::Wave(items) => add_wave(&mut svg_parts, items, end_time),
        State::Vec(x, items) => add_vec(&mut svg_parts, x, end_time, items),
    }
    wave.svg = Some(Svg {
        wave: format!(
            r#"<svg class="wave" data-id="{id}" data-size="{size}" transform="scale(10 2)" width="{width}">{body}</svg>"#,
            size = wave.var.size,
            // text = wave.var.reference,
            id = wave.var.code,
            body = svg_parts.concat(),
            width=end_time,
        ),
        bits: Vec::new(),
    })
}

fn size(v: &Value) -> usize {
    match v {
        Value::Scalar(_) => 1,
        Value::Vector(v) => v.len(),
    }
}

fn normalize(v: &mut Value) {
    if let Value::Vector(x) = v {
        if x.len() == 1 {
            *v = Value::Scalar(x[0]);
        }
    }
}

fn make_waves(items: &[ScopeItem]) -> BTreeMap<IdCode, Wave> {
    fn add_waves(waves: &mut BTreeMap<IdCode, Wave>, items: &[ScopeItem]) {
        for item in items {
            match item {
                ScopeItem::Var(var) => {
                    waves.insert(
                        var.code,
                        Wave {
                            var: var.clone(),
                            values: Vec::new(),
                            svg: None,
                        },
                    );
                }
                ScopeItem::Scope(scope) => {
                    add_waves(waves, &scope.children);
                }
            }
        }
    }
    let mut waves = BTreeMap::new();
    add_waves(&mut waves, items);
    waves
}

struct FmtVec<'a>(&'a [vcd::Value]);
impl<'a> std::fmt::Display for FmtVec<'a> {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        let value = self.0.iter().fold(0, |acc, item| {
            use vcd::Value::*;
            acc * 2 + match item {
                X | Z | V0 => 0,
                V1 => 1,
            }
        });
        write!(fmt, "{:#0width$x}", value, width = self.0.len() / 4)
    }
}
