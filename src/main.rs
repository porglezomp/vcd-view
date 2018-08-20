#![feature(nll)]

extern crate vcd;

use std::collections::BTreeMap;
use vcd::{Command, IdCode, Parser, ScopeItem, Var};

static HEADER: &str = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf8">
<style>
* { box-sizing: border-box; }
body { margin: 0; }
ul { padding-left: 10px; list-style: none; margin-top: 0; }
ul ul { padding-left: 20px; }
li.scope > .arrow {
  display: inline-block; width: 10px; height: 10px; border: solid black;
  border-width: 0 5px 5px 0; transform: rotate(45deg); margin: 0 2px 0 -12px;
}
li.scope.closed > .arrow { transform: rotate(-45deg); }
li.scope.closed ul { display: none; }
#container { height: 100vh; display: flex; }
#controls { flex: 0 0 auto; padding: 10px; overflow: scroll; width: 20vw; }
#display { flex: 0 0 auto; padding: 10px; overflow: scroll; width: 80vw; }
#display > svg { transform-origin: top left; overflow: visible; }
text { transform: scale(0.1, 0.5); text-anchor: middle; font-size: 10px; transform-origin: center center; }
polyline, rect { stroke-width: 2px; vector-effect: non-scaling-stroke; }
polyline { fill: none; stroke: black; }
rect.x { fill: #F66; stroke: #F00; }
rect.vec { fill: none; stroke: black; }
</style>
</head>
<body>
<div id="container">
<div id="controls">
<label for="scale">Scale</label>
<input type="text" id="scale" name="scale" value="10"/>"#;

static FOOTER: &str = r#"</svg>
</div>
</div>
<script>
document.querySelectorAll('.wave').forEach((elem, i) => {
  elem.setAttribute('transform', `translate(0 ${i * 15})`);
});

let textRule = null;
function findTextRule() {
  const sheets = document.styleSheets;
  let done = false;
  for (let i = 0; i < sheets.length && !done; i++) {
    const rules = sheets[i].cssRules;
    for (let j = 0; j < rules.length; j++) {
      if (rules[j].selectorText === 'text') {
        textRule = rules[j];
        done = true;
        break;
      }
    }
  }
}

function setScale(x, y) {
  if (!textRule) { findTextRule(); }
  const svg = document.querySelector('#display > svg');
  svg.setAttribute('transform', `scale(${x}, ${y})`);
  textRule.style.setProperty('transform', `scale(${1 / x}, ${1 / y})`);
}

function fixBBox() {
  const svg = document.querySelector('svg');
  let bbox = svg.getBBox();
  svg.setAttribute('width', bbox.width);
  svg.setAttribute('height', bbox.height);
}

document.querySelectorAll('.arrow').forEach(elt =>
  elt.addEventListener('click', event =>
    event.currentTarget.parentElement.classList.toggle('closed')));

setScale(10, 2);
fixBBox();
</script>
</body>
</html>"#;

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
            r#"<li class="var">
<label><input type="checkbox" data-id="{id}" checked/>{name}</label></li>"#,
            name = v.reference,
            id = v.code,
        );
    }
    fn print_scope(s: &vcd::Scope) {
        println!(
            r#"<li class="scope closed">
<div class="arrow"></div><label><input type="checkbox" data-name="{name}" checked/>{name}</label>
<ul>"#,
            name = s.identifier,
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
        match state {
            State::X(x) => add_undet(&mut svg_parts, x, end_time),
            State::Wave(items) => add_wave(&mut svg_parts, items, end_time),
            // Doesn't occur for a single element
            State::Vec(..) => (),
        }
        wave.svg = Some(Svg {
            wave: format!(
                r#"<g class="wave" data-id="{}" data-size="1">{}</g>"#,
                wave.var.code,
                svg_parts.concat(),
            ),
            bits: Vec::new(),
        })
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
        match state {
            State::X(x) => add_undet(&mut svg_parts, x, end_time),
            State::Vec(x, items) => add_vec(&mut svg_parts, x, end_time, items),
            // Doesn't occur for a vector
            State::Wave(..) => (),
        }
        wave.svg = Some(Svg {
            wave: format!(
                r#"<g class="wave" data-id="{}" data-size="{}">{}</g>"#,
                wave.var.code,
                wave.var.size,
                svg_parts.concat(),
            ),
            bits: Vec::new(),
        })
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
