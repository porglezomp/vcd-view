#![feature(nll)]

extern crate vcd;

use std::collections::BTreeMap;
use vcd::{Command, IdCode, Parser, ScopeItem, Var};

static HEADER: &str = r#"<!DOCTYPE html>
<html>
<head>
<style>
* { box-sizing: border-box; }
body { margin: 0; }
ul { list-style: none; padding-left: 20px; margin-top: 0; }
#container { height: 100vh; display: flex; }
#controls { flex: 0 0 auto; padding: 10px; overflow: scroll; width: 20vw; }
#display { flex: 0 0 auto; padding: 10px; overflow: scroll; width: 80vw; }
svg { transform-origin: top left; }
polyline { fill: none; stroke: black; vector-effect: non-scaling-stroke; }
rect.x { fill: #F66; stroke: #F00; rx: 0.5; ry: 0.5; }
rect.vec { fill: none; stroke: black; rx: 0.5; ry: 0.5; }
</style>
</head>
<body>
<div id="container">
<div id="controls">"#;

static FOOTER: &str = r#"</svg>
</div>
</div>
<script>
document.querySelectorAll('.wave').forEach((elem, i) => {
  elem.setAttribute('transform', `translate(0 ${i * 15})`);
});
</script>
</body>
</html>"#;

#[derive(Debug)]
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

    println!("{}", HEADER);
    print_vars(&header);
    println!(
        r#"</div>
<div id="display">
<svg transform="scale(10 2)" preserveAspectRatio="none" width="{}">"#,
        end_time
    );
    for wave in waves.values_mut() {
        render_svg(wave, end_time);
        if let Some(ref svg) = wave.svg {
            // println!("Wave {}:", wave.var.reference);
            println!("{}", svg.wave);
        }
    }
    println!("{}", FOOTER);

    Ok(())
}

fn print_vars(header: &vcd::Header) {
    fn print_var(v: &Var) {
        println!(
            r#"<li class="var" data-name="{}" data-id="{}">{}</li>"#,
            v.reference, v.code, v.reference
        );
    }
    fn print_scope(s: &vcd::Scope) {
        println!(
            r#"<li class="scope" data-name="{}">{}
<ul>"#,
            s.identifier, s.identifier
        );
        for child in &s.children {
            match child {
                ScopeItem::Var(var) => print_var(var),
                ScopeItem::Scope(scope) => print_scope(scope),
            }
        }
        println!(
            r#"</ul>
</li>"#
        );
    }
    println!("<ul>");
    for item in &header.items {
        match item {
            ScopeItem::Var(var) => print_var(var),
            ScopeItem::Scope(scope) => print_scope(scope),
        }
    }
    println!("</ul>");
}

fn render_svg(wave: &mut Wave, end_time: u64) {
    if wave.values.is_empty() {
        return;
    }

    enum State {
        Wave(Vec<(u64, bool)>),
        // Vec(u64, Vec<vcd::Value>),
        X(u64),
    }
    let mut svg_parts = Vec::new();

    fn add_undet(parts: &mut Vec<String>, start: u64, end: u64) {
        if start == end {
            return;
        }
        parts.push(format!(
            r#"<rect x="{}" y="0" width="{}" height="10"/>"#,
            start,
            end - start
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

    if wave.var.size == 1 {
        let mut state = State::X(0);
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
        match state {
            State::X(x) => add_undet(&mut svg_parts, x, end_time),
            State::Wave(items) => add_wave(&mut svg_parts, items, end_time),
        }
        wave.svg = Some(Svg {
            wave: format!(
                r#"<g class="wave" data-id="{}" data-size="1">{}</g>"#,
                wave.var.code,
                svg_parts.concat()
            ),
            bits: Vec::new(),
        })
    } else {
        // println!("Skipping wave {} ({})", wave.var.reference, wave.var.size);
    }
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