use crate::{Program, Type, expr::{Expr, Stmt}};

fn emit_expr(expr: &Expr, program: &Program) -> String {
    match expr {
        Expr::Number(value) => format!("Number::from_str_exact({value:?}).unwrap()"),
        Expr::String(value) => format!("String::from({value:?})"),
        Expr::Bool(value) => value.to_string(),
        Expr::Name(name) => name.clone(),
        Expr::Field(base, field) => {
            if let Expr::Name(name) = base.as_ref()
                && program.enums.iter().any(|item| item.name == *name)
            {
                return format!("{name}::{field}");
            }
            format!("({}.{}).clone()", emit_expr(base, program), field)
        }
        Expr::List(elements) => format!("vec![{}]", elements.iter()
            .map(|item| emit_expr(item, program)).collect::<Vec<_>>().join(", ")),
        Expr::Not(value) => format!("(!{})", emit_expr(value, program)),
        Expr::Binary(left, op, right) => {
            let operator = match op.as_str() { "and" => "&&", "or" => "||", other => other };
            format!("({} {operator} {})", emit_expr(left, program), emit_expr(right, program))
        }
    }
}

fn emit_stmt(stmt: &Stmt, program: &Program, out: &mut String) {
    match stmt {
        Stmt::Return(value) => out.push_str(&format!("return {};\n", emit_expr(value, program))),
        Stmt::If(condition, yes, no) => {
            out.push_str(&format!("if {} {{\n", emit_expr(condition, program)));
            for branch in yes { emit_stmt(branch, program, out); }
            out.push_str("}\n");
            if !no.is_empty() {
                out.push_str("else {\n");
                for branch in no { emit_stmt(branch, program, out); }
                out.push_str("}\n");
            }
        }
    }
}

fn rust_type(ty: &Type) -> String {
    match ty {
        Type::Named(name) => match name.as_str() {
            "Bool" => "bool".into(),
            "String" => "String".into(),
            _ => name.clone(),
        },
        Type::Generic(_, inner) => format!("Vec<{}>", rust_type(inner)),
    }
}

pub fn generate(program: &Program) -> Result<String, String> {
    let mut out = format!(
        "pub type Number = rust_decimal::Decimal;\npub const NAMESPACE: &str = {:?};\npub const VERSION: &str = {:?};\n",
        program.namespace, program.version,
    );
    for record in &program.records {
        out.push_str(&format!("#[derive(Debug, Clone, PartialEq)]\npub struct {} {{\n", record.name));
        for (field, ty) in &record.fields {
            out.push_str(&format!("  pub {field}: {},\n", rust_type(ty)));
        }
        out.push_str("}\n");
    }
    for item in &program.enums {
        out.push_str(&format!("#[allow(non_camel_case_types)]\n#[derive(Debug, Clone, PartialEq, Eq)]\npub enum {} {{\n", item.name));
        for variant in &item.variants {
            out.push_str(&format!("  {variant},\n"));
        }
        out.push_str("}\n");
    }
    for process in &program.processes {
        out.push_str(&format!(
            "pub fn {}({}: {}) -> {} {{\n",
            process.name, process.input, rust_type(&process.input_type), rust_type(&process.output),
        ));
        for stmt in &process.body {
            emit_stmt(stmt, program, &mut out);
        }
        out.push_str("}\n");
    }
    Ok(out)
}
