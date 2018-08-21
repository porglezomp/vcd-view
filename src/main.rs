#![feature(nll)]

extern crate vcd;

use std::collections::BTreeMap;
use vcd::{Command, IdCode, Parser, ScopeItem, Var};

mod svg;
mod webpage;

static WRAPPER: &str = include_str!("wrapper.html");

#[derive(Debug, PartialEq)]
enum Value {
    Scalar(vcd::Value),
    Vector(Vec<vcd::Value>),
}

#[derive(Debug)]
struct Wave {
    var: Var,
    values: Vec<(u64, Value)>,
    svg: Option<svg::Svg>,
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
        svg::render_svg(wave, end_time);
    }

    let vars_text = webpage::format_vars(&header).concat();
    let display_text =
        webpage::format_names(&header).concat() + &webpage::format_waves(&header, &waves).concat();
    let html = WRAPPER
        .replacen("$$$DISPLAY$$$", &display_text, 1)
        .replacen("$$$CONTROLS$$$", &vars_text, 1);
    println!("{}", html);

    Ok(())
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
