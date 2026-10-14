use crate::{
    Type,
    expr::{self, Expr},
    identifier, type_ref,
};

#[derive(Debug)]
pub struct DecisionModel {
    pub name: String,
    pub input: String,
    pub input_type: Type,
    pub output: Type,
    pub output_node: String,
    pub inputs: Vec<(String, Type)>,
    pub outputs: Vec<(String, Type, String)>,
    pub nodes: Vec<DecisionNode>,
    pub links: Vec<(String, String)>,
    pub knowledge: Vec<Knowledge>,
    pub braced: bool,
}

#[derive(Debug)]
pub struct DecisionNode {
    pub name: String,
    pub output: Type,
    pub output_name: String,
    pub kind: DecisionKind,
}

#[derive(Debug)]
pub enum DecisionKind {
    Literal(Expr),
    Context {
        entries: Vec<(String, Type, Expr)>,
        result: Expr,
    },
    Table(DecisionTable),
}

#[derive(Debug)]
pub struct DecisionTable {
    pub policy: String,
    pub aggregation: Option<String>,
    pub inputs: Vec<(String, Type, Expr)>,
    pub outputs: Vec<(String, Type)>,
    pub rules: Vec<(Expr, Vec<Expr>)>,
    pub priorities: Vec<Vec<Expr>>,
    pub default: Option<Vec<Expr>>,
}

fn expressions(text: &str) -> Result<Vec<Expr>, String> {
    expr::expression_list(text)
}

#[derive(Debug, Clone)]
pub struct Knowledge {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub output_name: String,
    pub output: Type,
    pub body: Expr,
    pub braced: bool,
}

fn infer_dependencies(
    expr: &mut Expr,
    target: &str,
    nodes: &[(String, String)],
    links: &mut Vec<(String, String)>,
) -> Result<(), String> {
    if let Expr::Field(value, port) = expr
        && let Expr::Name(source) = value.as_ref()
        && let Some((_, output_name)) = nodes.iter().find(|(name, _)| name == source)
    {
        if output_name != port {
            return Err(format!("unknown decision output port: {source}.{port}"));
        }
        *expr = Expr::Name(source.clone());
    }
    match expr {
        Expr::Name(source) if nodes.iter().any(|(name, _)| name == source) => {
            let edge = (source.clone(), target.into());
            if !links.contains(&edge) {
                links.push(edge);
            }
        }
        Expr::Field(value, _) | Expr::Not(value) => {
            infer_dependencies(value, target, nodes, links)?
        }
        Expr::InlinePredicate { binding, body } => {
            let scoped: Vec<_> = nodes
                .iter()
                .filter(|(name, _)| name != binding)
                .cloned()
                .collect();
            infer_dependencies(body, target, &scoped, links)?;
        }
        Expr::Binary(left, _, right) => {
            infer_dependencies(left, target, nodes, links)?;
            infer_dependencies(right, target, nodes, links)?;
        }
        Expr::Call(name, args) if name == "__bl_dictionary" => {
            let mut scoped = Vec::new();
            for entry in args.chunks_exact_mut(2) {
                let available: Vec<_> = nodes
                    .iter()
                    .filter(|(name, _)| !scoped.contains(name))
                    .cloned()
                    .collect();
                infer_dependencies(&mut entry[1], target, &available, links)?;
                if let Expr::String(key) = &entry[0]
                    && identifier(key)
                {
                    scoped.push(key.clone());
                }
            }
        }
        Expr::Call(_, args) | Expr::List(args) => {
            for arg in args {
                infer_dependencies(arg, target, nodes, links)?;
            }
        }
        Expr::Iteration {
            binding,
            source,
            body,
            ..
        } => {
            infer_dependencies(source, target, nodes, links)?;
            let scoped: Vec<_> = nodes
                .iter()
                .filter(|(name, _)| name != binding)
                .cloned()
                .collect();
            infer_dependencies(body, target, &scoped, links)?;
        }
        Expr::Range(start, end, _, _) => {
            if let Some(start) = start {
                infer_dependencies(start, target, nodes, links)?;
            }
            if let Some(end) = end {
                infer_dependencies(end, target, nodes, links)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_braced_table(body: &[String]) -> Result<(String, Type, DecisionTable), String> {
    let output = body
        .first()
        .and_then(|part| part.strip_prefix("output "))
        .and_then(|part| part.split_once(": "))
        .ok_or("decision table requires a typed result port first")?;
    if !identifier(output.0) {
        return Err("invalid decision table result port".into());
    }
    let mut table = DecisionTable {
        policy: String::new(),
        aggregation: None,
        inputs: vec![],
        outputs: vec![],
        rules: vec![],
        priorities: vec![],
        default: None,
    };
    for part in &body[1..] {
        if let Some(policy) = part.strip_prefix("policy ") {
            if !table.policy.is_empty() {
                return Err("duplicate table policy".into());
            }
            let mut words = policy.split_whitespace();
            table.policy = words.next().unwrap_or_default().into();
            table.aggregation = words.next().map(str::to_owned);
            if words.next().is_some()
                || !matches!(
                    table.policy.as_str(),
                    "UNIQUE"
                        | "ANY"
                        | "FIRST"
                        | "PRIORITY"
                        | "RULE_ORDER"
                        | "OUTPUT_ORDER"
                        | "COLLECT"
                )
            {
                return Err("invalid table policy".into());
            }
        } else if let Some(text) = part.strip_prefix("input ") {
            let (column, expression) = text.split_once(" = ").ok_or("invalid table input")?;
            let (name, ty) = column.split_once(": ").ok_or("invalid table input")?;
            if !identifier(name) {
                return Err("invalid table input".into());
            }
            table
                .inputs
                .push((name.into(), type_ref(ty)?, expr::expression(expression)?));
        } else if let Some(text) = part.strip_prefix("output ") {
            let (name, ty) = text.split_once(": ").ok_or("invalid table output")?;
            if !identifier(name) {
                return Err("invalid table output".into());
            }
            table.outputs.push((name.into(), type_ref(ty)?));
        } else if let Some(text) = part.strip_prefix("rule ") {
            let (condition, values) = text.split_once(" -> ").ok_or("invalid table rule")?;
            table.rules.push((
                expr::table_condition(
                    condition,
                    &table
                        .inputs
                        .iter()
                        .map(|(name, _, _)| name.clone())
                        .collect::<Vec<_>>(),
                )?,
                expressions(values)?,
            ));
        } else if let Some(text) = part.strip_prefix("priority ") {
            table.priorities.push(expressions(text)?);
        } else if let Some(text) = part.strip_prefix("default ") {
            if table.default.replace(expressions(text)?).is_some() {
                return Err("duplicate table default".into());
            }
        } else {
            return Err(format!("invalid decision table statement: {part}"));
        }
    }
    if table.policy.is_empty() {
        return Err("missing table policy".into());
    }
    Ok((output.0.into(), type_ref(output.1)?, table))
}

fn parse_braced_context(node: &str, body: &[String]) -> Result<DecisionNode, String> {
    let mut output = None;
    let mut entries = Vec::new();
    let mut result = None;
    for part in body {
        if let Some(text) = part.strip_prefix("output ") {
            let (port, ty) = text.split_once(": ").ok_or("invalid context output")?;
            if !identifier(port) || output.replace((port.into(), type_ref(ty)?)).is_some() {
                return Err("invalid context output".into());
            }
        } else if let Some(text) = part.strip_prefix("entry ") {
            let (field, value) = text.split_once(" = ").ok_or("invalid context entry")?;
            let (port, ty) = field.split_once(": ").ok_or("invalid context entry")?;
            if !identifier(port) {
                return Err("invalid context entry".into());
            }
            entries.push((port.into(), type_ref(ty)?, expr::expression(value)?));
        } else if let Some(text) = part.strip_prefix("result ") {
            if result.replace(expr::expression(text)?).is_some() {
                return Err("duplicate context result".into());
            }
        } else {
            return Err(format!("invalid context statement: {part}"));
        }
    }
    let (output_name, output) = output.ok_or("missing context output")?;
    Ok(DecisionNode {
        name: node.into(),
        output,
        output_name,
        kind: DecisionKind::Context {
            entries,
            result: result.ok_or("missing context result")?,
        },
    })
}

fn sort_context_entries(entries: &mut Vec<(String, Type, Expr)>) -> Result<(), String> {
    let names: std::collections::HashSet<_> =
        entries.iter().map(|(name, _, _)| name.as_str()).collect();
    if names.len() != entries.len() {
        return Err("duplicate context entry".into());
    }
    let names: std::collections::HashSet<String> = names.into_iter().map(str::to_owned).collect();
    let mut pending = std::mem::take(entries);
    let mut done = std::collections::HashSet::new();
    while !pending.is_empty() {
        let index = pending
            .iter()
            .position(|(_, _, expr)| {
                let mut referenced = Vec::new();
                expr_names(expr, &mut referenced);
                referenced
                    .into_iter()
                    .all(|name| !names.contains(name) || done.contains(name))
            })
            .ok_or("context cycle")?;
        let entry = pending.remove(index);
        done.insert(entry.0.clone());
        entries.push(entry);
    }
    Ok(())
}

fn expr_calls<'a>(expr: &'a Expr, found: &mut Vec<&'a str>) {
    match expr {
        Expr::Call(name, args) => {
            found.push(name);
            for arg in args {
                expr_calls(arg, found);
            }
        }
        Expr::Field(value, _) | Expr::Not(value) => expr_calls(value, found),
        Expr::InlinePredicate { body, .. } => expr_calls(body, found),
        Expr::Binary(left, _, right) => {
            expr_calls(left, found);
            expr_calls(right, found);
        }
        Expr::List(args) => {
            for arg in args {
                expr_calls(arg, found);
            }
        }
        Expr::Iteration { source, body, .. } => {
            expr_calls(source, found);
            expr_calls(body, found);
        }
        Expr::Range(lower, upper, _, _) => {
            for bound in lower.iter().chain(upper.iter()) {
                expr_calls(bound, found);
            }
        }
        _ => {}
    }
}

fn node_expressions(node: &DecisionNode) -> Vec<&Expr> {
    match &node.kind {
        DecisionKind::Literal(expr) => vec![expr],
        DecisionKind::Context { entries, result } => entries
            .iter()
            .map(|(_, _, expr)| expr)
            .chain(std::iter::once(result))
            .collect(),
        DecisionKind::Table(table) => table
            .inputs
            .iter()
            .map(|(_, _, expr)| expr)
            .chain(
                table
                    .rules
                    .iter()
                    .flat_map(|(condition, values)| std::iter::once(condition).chain(values)),
            )
            .chain(table.priorities.iter().flat_map(|values| values.iter()))
            .chain(table.default.iter().flat_map(|values| values.iter()))
            .collect(),
    }
}

fn expr_names<'a>(expr: &'a Expr, found: &mut Vec<&'a str>) {
    match expr {
        Expr::Name(name) => found.push(name),
        Expr::Field(value, _) | Expr::Not(value) => expr_names(value, found),
        Expr::InlinePredicate { binding, body } => {
            let mut local = Vec::new();
            expr_names(body, &mut local);
            found.extend(local.into_iter().filter(|name| *name != binding));
        }
        Expr::Binary(left, _, right) => {
            expr_names(left, found);
            expr_names(right, found);
        }
        Expr::Call(_, args) | Expr::List(args) => {
            for arg in args {
                expr_names(arg, found);
            }
        }
        Expr::Iteration {
            binding,
            source,
            body,
            ..
        } => {
            expr_names(source, found);
            let mut local = Vec::new();
            expr_names(body, &mut local);
            found.extend(local.into_iter().filter(|name| *name != binding));
        }
        Expr::Range(lower, upper, _, _) => {
            for bound in lower.iter().chain(upper.iter()) {
                expr_names(bound, found);
            }
        }
        _ => {}
    }
}

pub fn parse_braced(name: &str, items: &[String]) -> Result<DecisionModel, String> {
    let mut inputs: Vec<(String, Type)> = Vec::new();
    let mut outputs: Vec<(String, Type, String)> = Vec::new();
    let mut nodes = Vec::new();
    let mut knowledge = Vec::new();
    let mut i = 0;
    while i < items.len() {
        let statement = &items[i];
        if let Some(text) = statement.strip_prefix("input ") {
            let (port, ty) = text.split_once(": ").ok_or("invalid decision input")?;
            if !identifier(port) {
                return Err("invalid decision input".into());
            }
            inputs.push((port.into(), type_ref(ty)?));
            i += 1;
        } else if let Some(text) = statement.strip_prefix("output ") {
            let (port, value) = text.split_once(": ").ok_or("invalid decision output")?;
            let (ty, node) = value.split_once(" = ").ok_or("invalid decision output")?;
            if !identifier(port) || node.is_empty() {
                return Err("invalid decision output".into());
            }
            outputs.push((port.into(), type_ref(ty)?, node.into()));
            i += 1;
        } else if let Some(node) = statement.strip_prefix("literal_expression ") {
            if !identifier(node) {
                return Err("invalid literal expression name".into());
            }
            i += 1;
            let body = crate::compiler::block_items(items, &mut i)?;
            let mut output = None;
            let mut expression = None;
            for part in &body {
                if let Some(text) = part.strip_prefix("output ") {
                    let (port, ty) = text.split_once(": ").ok_or("invalid literal output")?;
                    if !identifier(port) || output.is_some() {
                        return Err("invalid literal output".into());
                    }
                    output = Some((port.to_owned(), type_ref(ty)?));
                } else if let Some(text) = part.strip_prefix("expression ") {
                    if expression.is_some() {
                        return Err("duplicate literal expression".into());
                    }
                    expression = Some(expr::expression(text)?);
                } else {
                    return Err(format!("invalid literal statement: {part}"));
                }
            }
            let (output_name, output) = output.ok_or("missing literal output")?;
            nodes.push(DecisionNode {
                name: node.into(),
                output,
                output_name,
                kind: DecisionKind::Literal(expression.ok_or("missing literal expression")?),
            });
        } else if let Some(node) = statement.strip_prefix("context ") {
            if !identifier(node) {
                return Err("invalid context name".into());
            }
            i += 1;
            let body = crate::compiler::block_items(items, &mut i)?;
            nodes.push(parse_braced_context(node, &body)?);
        } else if let Some(model) = statement.strip_prefix("knowledge ") {
            if !identifier(model) {
                return Err("invalid knowledge name".into());
            }
            i += 1;
            let body = crate::compiler::block_items(items, &mut i)?;
            let mut params = Vec::new();
            let mut output = None;
            let mut expression = None;
            for part in &body {
                if let Some(text) = part.strip_prefix("input ") {
                    let (port, ty) = text.split_once(": ").ok_or("invalid knowledge input")?;
                    if !identifier(port) || params.iter().any(|(name, _)| name == port) {
                        return Err("invalid knowledge input".into());
                    }
                    params.push((port.into(), type_ref(ty)?));
                } else if let Some(text) = part.strip_prefix("output ") {
                    let (port, ty) = text.split_once(": ").ok_or("invalid knowledge output")?;
                    if !identifier(port)
                        || output.replace((port.to_owned(), type_ref(ty)?)).is_some()
                    {
                        return Err("invalid knowledge output".into());
                    }
                } else if let Some(text) = part.strip_prefix("expression ") {
                    if expression.replace(expr::expression(text)?).is_some() {
                        return Err("duplicate knowledge expression".into());
                    }
                } else {
                    return Err(format!("invalid knowledge statement: {part}"));
                }
            }
            let (output_name, output) = output.ok_or("missing knowledge output")?;
            knowledge.push(Knowledge {
                name: model.into(),
                params,
                output_name,
                output,
                body: expression.ok_or("missing knowledge expression")?,
                braced: true,
            });
        } else if let Some(node) = statement.strip_prefix("decision_table ") {
            if !identifier(node) {
                return Err("invalid decision table name".into());
            }
            i += 1;
            let body = crate::compiler::block_items(items, &mut i)?;
            let (output_name, output, table) = parse_braced_table(&body)?;
            nodes.push(DecisionNode {
                name: node.into(),
                output,
                output_name,
                kind: DecisionKind::Table(table),
            });
        } else {
            return Err(format!("invalid decision task statement: {statement}"));
        }
    }
    finish_braced(name, inputs, outputs, nodes, knowledge)
}

fn finish_braced(
    name: &str,
    inputs: Vec<(String, Type)>,
    outputs: Vec<(String, Type, String)>,
    mut nodes: Vec<DecisionNode>,
    mut knowledge: Vec<Knowledge>,
) -> Result<DecisionModel, String> {
    let (input, input_type) = inputs
        .first()
        .ok_or("decision task requires an input port")?;
    let (_, output, node) = outputs
        .first()
        .ok_or("decision task requires an output port")?;
    let mut names = std::collections::HashSet::new();
    for (port, _) in &inputs {
        if !names.insert(port.as_str()) {
            return Err(format!("duplicate decision input: {port}"));
        }
    }
    for decision in &nodes {
        if !names.insert(&decision.name) {
            return Err(format!("duplicate decision name: {}", decision.name));
        }
    }
    let mut ports = std::collections::HashSet::new();
    for (port, ty, reference) in &outputs {
        if !ports.insert(port.as_str()) {
            return Err(format!("duplicate decision output: {port}"));
        }
        let (node_name, output_port) = reference
            .split_once('.')
            .map_or((reference.as_str(), None), |(node, port)| {
                (node, Some(port))
            });
        let decision = nodes
            .iter()
            .find(|item| item.name == node_name)
            .ok_or_else(|| format!("unknown decision node: {node_name}"))?;
        if output_port.is_some_and(|output_port| output_port != decision.output_name) {
            return Err(format!("unknown decision output port: {reference}"));
        }
        if ty != &decision.output {
            return Err(format!("decision output type mismatch: {port}"));
        }
    }
    let references: Vec<_> = nodes
        .iter()
        .map(|node| (node.name.clone(), node.output_name.clone()))
        .collect();
    let mut captures = Vec::new();
    for model in &mut knowledge {
        infer_dependencies(&mut model.body, &model.name, &references, &mut captures)?;
    }
    let mut links = Vec::new();
    for node in &mut nodes {
        match &mut node.kind {
            DecisionKind::Literal(expr) => {
                infer_dependencies(expr, &node.name, &references, &mut links)?
            }
            DecisionKind::Table(table) => {
                for (_, _, expr) in &mut table.inputs {
                    infer_dependencies(expr, &node.name, &references, &mut links)?;
                }
                for (condition, values) in &mut table.rules {
                    infer_dependencies(condition, &node.name, &references, &mut links)?;
                    for expr in values {
                        infer_dependencies(expr, &node.name, &references, &mut links)?;
                    }
                }
                for values in &mut table.priorities {
                    for expr in values {
                        infer_dependencies(expr, &node.name, &references, &mut links)?;
                    }
                }
                if let Some(values) = &mut table.default {
                    for expr in values {
                        infer_dependencies(expr, &node.name, &references, &mut links)?;
                    }
                }
            }
            DecisionKind::Context { entries, result } => {
                for (_, _, expr) in entries.iter_mut() {
                    infer_dependencies(expr, &node.name, &references, &mut links)?;
                }
                infer_dependencies(result, &node.name, &references, &mut links)?;
                sort_context_entries(entries)?;
            }
        }
    }
    for node in &nodes {
        let mut pending = Vec::new();
        for expr in node_expressions(node) {
            expr_calls(expr, &mut pending);
        }
        let mut visited = std::collections::HashSet::new();
        while let Some(name) = pending.pop() {
            if let Some(model) = knowledge.iter().find(|model| model.name == name)
                && visited.insert(name)
            {
                for (source, _) in captures.iter().filter(|(_, target)| target == name) {
                    let edge = (source.clone(), node.name.clone());
                    if !links.contains(&edge) {
                        links.push(edge);
                    }
                }
                expr_calls(&model.body, &mut pending);
            }
        }
    }
    Ok(DecisionModel {
        name: name.into(),
        input: input.clone(),
        input_type: input_type.clone(),
        output: output.clone(),
        output_node: node.split('.').next().unwrap().into(),
        inputs,
        outputs,
        nodes,
        links,
        knowledge,
        braced: true,
    })
}

#[derive(Debug)]
enum DecisionStatement {
    Input(String, Type),
    TableInput(String, Type, Expr),
    Output(String, Type, Option<(String, Option<String>)>),
    Expression(Expr),
    Entry(String, Type, Expr),
    Result(Expr),
    Policy(String, Option<String>),
    Rule(Expr, Vec<Expr>),
    Priority(Vec<Expr>),
    Default(Vec<Expr>),
}

fn decision_statement<'a>(
    source: &'a str,
    columns: &'a [String],
) -> impl chumsky::Parser<
    'a,
    crate::compiler::Tokens<'a>,
    DecisionStatement,
    crate::compiler::ParseError<'a>,
> {
    use crate::{
        compiler::{assignment, name, typed_port, word},
        lexer::Token,
    };
    use chumsky::prelude::*;
    let name = name(source);
    let typed = typed_port(source);
    let expression = expr::expression_parser(source, &[]).boxed();
    let condition = expr::expression_parser(source, columns).boxed();
    let values = expression
        .clone()
        .separated_by(just(Token::Comma))
        .at_least(1)
        .collect::<Vec<_>>();
    let input = word(source, "input").ignore_then(choice((
        assignment(source, typed.clone(), expression.clone())
            .map(|(port, expr)| DecisionStatement::TableInput(port.0, port.1, expr)),
        typed
            .clone()
            .map(|(port, ty)| DecisionStatement::Input(port, ty)),
    )));
    let reference = name
        .clone()
        .map_with(|value, extra| (value, extra.span()))
        .then(
            just(Token::Dot)
                .map_with(|_, extra| extra.span())
                .then(name.clone().map_with(|value, extra| (value, extra.span())))
                .or_not(),
        )
        .try_map_with(|((node, node_span), port), extra| {
            if let Some((dot, (port, port_span))) = port {
                if node_span.end != dot.start || dot.end != port_span.start {
                    return Err(Rich::custom(
                        extra.span(),
                        "invalid decision output reference",
                    ));
                }
                Ok((node, Some(port)))
            } else {
                Ok((node, None))
            }
        });
    let output = word(source, "output").ignore_then(choice((
        assignment(source, typed.clone(), reference)
            .map(|((port, ty), reference)| DecisionStatement::Output(port, ty, Some(reference))),
        typed
            .clone()
            .map(|(port, ty)| DecisionStatement::Output(port, ty, None)),
    )));
    let entry = word(source, "entry")
        .ignore_then(assignment(source, typed, expression.clone()))
        .map(|((name, ty), expression)| DecisionStatement::Entry(name, ty, expression));
    let expression_stmt = word(source, "expression")
        .ignore_then(expression.clone())
        .map(DecisionStatement::Expression);
    let result = word(source, "result")
        .ignore_then(expression.clone())
        .map(DecisionStatement::Result);
    let policy = word(source, "policy")
        .ignore_then(name.clone())
        .then(name.or_not())
        .map(|(policy, aggregation)| DecisionStatement::Policy(policy, aggregation));
    let rule = word(source, "rule")
        .ignore_then(crate::graph::edge(source, condition, values.clone()))
        .map(|(condition, values)| DecisionStatement::Rule(condition, values));
    let priority = word(source, "priority")
        .ignore_then(values.clone())
        .map(DecisionStatement::Priority);
    let default = word(source, "default")
        .ignore_then(values)
        .map(DecisionStatement::Default);
    choice((
        input.boxed(),
        output.boxed(),
        entry.boxed(),
        expression_stmt.boxed(),
        result.boxed(),
        policy.boxed(),
        rule.boxed(),
        priority.boxed(),
        default.boxed(),
    ))
}

fn parse_item(
    source: &str,
    item: &crate::compiler::BodyItem,
    columns: &[String],
) -> Result<DecisionStatement, crate::compiler::CompilerError> {
    let (tokens, terminator) = match item {
        crate::compiler::BodyItem::Statement(tokens, terminator) => (tokens, terminator),
        crate::compiler::BodyItem::Block(.., kind_span) => {
            return Err(crate::compiler::CompilerError::located(
                "unexpected nested decision block",
                kind_span.into_range(),
            ));
        }
        crate::compiler::BodyItem::Invalid => return Err("invalid decision statement".into()),
    };
    crate::compiler::statement(tokens, *terminator, decision_statement(source, columns)).map_err(
        |error| {
            if tokens
                .first()
                .is_some_and(|(_, span)| &source[span.into_range()] == "expression")
            {
                error.context("expression")
            } else {
                error
            }
        },
    )
}

fn node_tokens(
    source: &str,
    kind: &str,
    name: &str,
    body: &[crate::compiler::BodyItem],
) -> Result<Option<Result<DecisionNode, Knowledge>>, crate::compiler::CompilerError> {
    let mut output = None;
    let mut expression = None;
    let mut entries = Vec::new();
    let mut params = Vec::new();
    let mut result = None;
    let mut table = DecisionTable {
        policy: String::new(),
        aggregation: None,
        inputs: vec![],
        outputs: vec![],
        rules: vec![],
        priorities: vec![],
        default: None,
    };
    let mut errors: Option<crate::compiler::CompilerError> = None;
    let mut damaged = false;
    for (index, item) in body.iter().enumerate() {
        if matches!(item, crate::compiler::BodyItem::Invalid) {
            damaged = true;
            continue;
        }
        let columns = table
            .inputs
            .iter()
            .map(|(name, _, _): &(String, Type, Expr)| name.clone())
            .collect::<Vec<_>>();
        let statement = match parse_item(source, item, &columns) {
            Ok(statement) => statement,
            Err(error) => {
                if let Some(errors) = &mut errors {
                    errors.append(error);
                } else {
                    errors = Some(error);
                }
                continue;
            }
        };
        match (kind, statement) {
            (
                "literal_expression" | "knowledge" | "context",
                DecisionStatement::Output(port, ty, None),
            ) if output.is_none() => {
                output = Some((port, ty));
            }
            ("decision_table", DecisionStatement::Output(port, ty, None))
                if index == 0 && output.is_none() =>
            {
                output = Some((port, ty));
            }
            ("decision_table", DecisionStatement::Output(port, ty, None)) if index > 0 => {
                table.outputs.push((port, ty))
            }
            ("knowledge", DecisionStatement::Input(port, ty))
                if !params
                    .iter()
                    .any(|(name, _): &(String, Type)| name == &port) =>
            {
                params.push((port, ty))
            }
            ("literal_expression" | "knowledge", DecisionStatement::Expression(expr))
                if expression.is_none() =>
            {
                expression = Some(expr);
            }
            ("context", DecisionStatement::Entry(port, ty, expr)) => entries.push((port, ty, expr)),
            ("context", DecisionStatement::Result(expr)) if result.is_none() => {
                result = Some(expr);
            }
            ("decision_table", DecisionStatement::TableInput(port, ty, expr)) => {
                table.inputs.push((port, ty, expr))
            }
            ("decision_table", DecisionStatement::Policy(policy, aggregation))
                if table.policy.is_empty()
                    && matches!(
                        policy.as_str(),
                        "UNIQUE"
                            | "ANY"
                            | "FIRST"
                            | "PRIORITY"
                            | "RULE_ORDER"
                            | "OUTPUT_ORDER"
                            | "COLLECT"
                    ) =>
            {
                table.policy = policy;
                table.aggregation = aggregation;
            }
            ("decision_table", DecisionStatement::Rule(condition, values)) => {
                table.rules.push((condition, values))
            }
            ("decision_table", DecisionStatement::Priority(values)) => {
                table.priorities.push(values)
            }
            ("decision_table", DecisionStatement::Default(values)) if table.default.is_none() => {
                table.default = Some(values);
            }
            _ => return Err(format!("invalid {kind} statement").into()),
        }
    }
    if let Some(error) = errors {
        return Err(error);
    }
    if damaged {
        return Ok(None);
    }
    let (output_name, output) = output.ok_or_else(|| format!("missing {kind} output"))?;
    match kind {
        "knowledge" => Ok(Some(Err(Knowledge {
            name: name.into(),
            params,
            output_name,
            output,
            body: expression.ok_or("missing knowledge expression")?,
            braced: true,
        }))),
        "literal_expression" => Ok(Some(Ok(DecisionNode {
            name: name.into(),
            output,
            output_name,
            kind: DecisionKind::Literal(expression.ok_or("missing literal expression")?),
        }))),
        "context" => Ok(Some(Ok(DecisionNode {
            name: name.into(),
            output,
            output_name,
            kind: DecisionKind::Context {
                entries,
                result: result.ok_or("missing context result")?,
            },
        }))),
        "decision_table" if !table.policy.is_empty() => Ok(Some(Ok(DecisionNode {
            name: name.into(),
            output,
            output_name,
            kind: DecisionKind::Table(table),
        }))),
        "decision_table" => Err("missing table policy".into()),
        _ => Err(format!("invalid decision block: {kind}").into()),
    }
}

pub(crate) fn parse_tokens(
    source: &str,
    name: &str,
    items: &[crate::compiler::BodyItem],
) -> Result<Option<DecisionModel>, crate::compiler::CompilerError> {
    let (mut inputs, mut outputs, mut nodes, mut knowledge) = (vec![], vec![], vec![], vec![]);
    let mut errors: Option<crate::compiler::CompilerError> = None;
    let mut damaged = false;
    for item in items {
        let result = match item {
            crate::compiler::BodyItem::Invalid => {
                damaged = true;
                continue;
            }
            crate::compiler::BodyItem::Statement(..) => {
                parse_item(source, item, &[]).and_then(|statement| {
                    match statement {
                        DecisionStatement::Input(port, ty) => inputs.push((port, ty)),
                        DecisionStatement::Output(port, ty, Some((node, field))) => outputs.push((
                            port,
                            ty,
                            field.map_or(node.clone(), |field| format!("{node}.{field}")),
                        )),
                        _ => return Err("invalid decision task statement".into()),
                    }
                    Ok(())
                })
            }
            crate::compiler::BodyItem::Block(kind, name, body, _) => {
                node_tokens(source, kind, name, body).map(|node| match node {
                    Some(Ok(node)) => nodes.push(node),
                    Some(Err(model)) => knowledge.push(model),
                    None => damaged = true,
                })
            }
        };
        if let Err(error) = result {
            if let Some(errors) = &mut errors {
                errors.append(error);
            } else {
                errors = Some(error);
            }
        }
    }
    if let Some(error) = errors {
        return Err(error);
    }
    if damaged {
        return Ok(None);
    }
    finish_braced(name, inputs, outputs, nodes, knowledge)
        .map(Some)
        .map_err(Into::into)
}
