use super::Wave;
use std::collections::BTreeMap;
use svg::Svg;
use vcd::{self, IdCode, ScopeItem, Var};

fn walk_dfs<State>(
    open: impl Fn() -> State,
    open_scope: impl Fn(&mut State, &vcd::Scope),
    do_var: impl Fn(&mut State, &Var),
    close_scope: impl Fn(&mut State, &vcd::Scope),
    close: impl Fn(&mut State),
    header: &vcd::Header,
) -> State {
    fn walk_scope<State>(
        open_scope: &impl Fn(&mut State, &vcd::Scope),
        do_var: &impl Fn(&mut State, &Var),
        close_scope: &impl Fn(&mut State, &vcd::Scope),
        state: &mut State,
        s: &vcd::Scope,
    ) {
        open_scope(state, s);
        for child in &s.children {
            match child {
                ScopeItem::Var(var) => do_var(state, var),
                ScopeItem::Scope(s) => walk_scope(open_scope, do_var, close_scope, state, s),
            }
        }
        close_scope(state, s);
    }
    let mut state = open();
    for item in &header.items {
        match item {
            ScopeItem::Var(var) => do_var(&mut state, var),
            ScopeItem::Scope(scope) => {
                walk_scope(&open_scope, &do_var, &close_scope, &mut state, scope)
            }
        }
    }
    close(&mut state);
    state
}

pub(crate) fn format_vars(header: &vcd::Header) -> Vec<String> {
    walk_dfs(
        || vec!["<ul>".into()],
        |a, s| {
            a.push(format!(
                r#"<li class="scope closed">
<div class="arrow"></div><label><input class="scope-checkbox" type="checkbox" data-name="{name}" checked/>{name}</label>
<ul>"#,
                name = s.identifier,
            ));
        },
        |a, v| {
            a.push(format!(
                r#"<li class="var">
<label><input type="checkbox" data-id="{id}" checked/>{name}</label></li>"#,
                id = v.code,
                name = v.reference,
            ))
        },
        |a, _| a.push("</ul>\n</li>".into()),
        |a| a.push("</ul>".into()),
        header,
    )
}

pub(crate) fn format_names(header: &vcd::Header) -> Vec<String> {
    walk_dfs(
        || vec![r#"<div id="labels"><ul>"#.into()],
        |_, _| (),
        |a, v| {
            a.push(format!(
                r#"<li data-id="{id}">{name}</li>"#,
                id = v.code,
                name = v.reference
            ))
        },
        |_, _| (),
        |a| a.push("</ul></div>".into()),
        header,
    )
}

pub(crate) fn format_waves(header: &vcd::Header, waves: &BTreeMap<IdCode, Wave>) -> Vec<String> {
    let wave_count: usize = waves
        .values()
        .map(|wave| match wave.svg {
            Some(Svg { ref bits, .. }) => 1 + bits.len(),
            _ => 0,
        }).sum();
    let height = wave_count * 40 + 20;
    walk_dfs(
        || {
            vec![format!(
                r#"<div id="waves" style="height: {}px;"><ul>"#,
                height
            )]
        },
        |_, _| (),
        |a, v| {
            if let Some(ref svg) = waves[&v.code].svg {
                a.push(format!(
                    r#"<li data-id="{id}">{wave}</li>"#,
                    id = v.code,
                    wave = svg.wave
                ));
            }
        },
        |_, _| (),
        |a| a.push("</ul></div>".into()),
        header,
    )
}
