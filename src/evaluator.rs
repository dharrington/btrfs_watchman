use serde_bser::value::Value;

#[derive(Debug, Clone)]
pub enum CompiledExpr {
    Not(Box<CompiledExpr>),
    AnyOf(Vec<CompiledExpr>),
    AllOf(Vec<CompiledExpr>),
    DirName {
        dir: String,
        starts_with: String,
        contains: String,
    },
    Name {
        names: Vec<String>,
        starts_withs: Vec<String>,
        is_wholename: bool,
    },
    True,
}

fn extract_string(val: Option<&Value>) -> Option<String> {
    match val {
        Some(Value::Utf8String(s)) => Some(s.clone()),
        Some(Value::ByteString(b)) => String::from_utf8(b.to_vec()).ok(),
        _ => None,
    }
}

// Note: invalid expressions will evaluate to true.
pub fn parse_expr(expr: &Value) -> CompiledExpr {
    match expr {
        Value::Array(arr) => {
            if arr.is_empty() {
                return CompiledExpr::True;
            }
            if let Some(op) = extract_string(arr.get(0)) {
                match op.as_str() {
                    "not" => {
                        if arr.len() > 1 {
                            return CompiledExpr::Not(Box::new(parse_expr(&arr[1])));
                        }
                    }
                    "anyof" => {
                        let mut subs = Vec::new();
                        for subexpr in arr.iter().skip(1) {
                            subs.push(parse_expr(subexpr));
                        }
                        return CompiledExpr::AnyOf(subs);
                    }
                    "allof" => {
                        let mut subs = Vec::new();
                        for subexpr in arr.iter().skip(1) {
                            subs.push(parse_expr(subexpr));
                        }
                        return CompiledExpr::AllOf(subs);
                    }
                    "dirname" => {
                        if let Some(dir) = extract_string(arr.get(1)) {
                            let starts_with = format!("{}/", dir);
                            let contains = format!("/{}/", dir);
                            return CompiledExpr::DirName {
                                dir,
                                starts_with,
                                contains,
                            };
                        }
                    }
                    "name" => {
                        let mut is_wholename = false;
                        if let Some(s) = extract_string(arr.get(2)) {
                            if s == "wholename" {
                                is_wholename = true;
                            }
                        }

                        let names = match arr.get(1) {
                            Some(Value::Array(n)) => n
                                .iter()
                                .filter_map(|v| extract_string(Some(v)))
                                .collect::<Vec<_>>(),
                            Some(val) => {
                                if let Some(s) = extract_string(Some(val)) {
                                    vec![s]
                                } else {
                                    vec![]
                                }
                            }
                            None => vec![],
                        };

                        let starts_withs = if is_wholename {
                            names.iter().map(|n| format!("{}/", n)).collect()
                        } else {
                            Vec::new()
                        };

                        return CompiledExpr::Name {
                            names,
                            starts_withs,
                            is_wholename,
                        };
                    }
                    _ => {}
                }
            }
            CompiledExpr::True
        }
        _ => CompiledExpr::True,
    }
}

impl CompiledExpr {
    pub fn evaluate(&self, path: &str) -> bool {
        match self {
            CompiledExpr::Not(inner) => !inner.evaluate(path),
            CompiledExpr::AnyOf(exprs) => {
                for expr in exprs {
                    if expr.evaluate(path) {
                        return true;
                    }
                }
                false
            }
            CompiledExpr::AllOf(exprs) => {
                for expr in exprs {
                    if !expr.evaluate(path) {
                        return false;
                    }
                }
                true
            }
            CompiledExpr::DirName {
                dir,
                starts_with,
                contains,
            } => path == dir || path.starts_with(starts_with) || path.contains(contains),
            CompiledExpr::Name {
                names,
                starts_withs,
                is_wholename,
            } => {
                if *is_wholename {
                    for i in 0..names.len() {
                        if path == &names[i] || path.starts_with(&starts_withs[i]) {
                            return true;
                        }
                    }
                } else {
                    if let Some(last) = path.rsplit('/').next() {
                        for name in names {
                            if last == name {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            CompiledExpr::True => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to build a BSER Value from strings
    fn build_array(items: Vec<Value>) -> Value {
        Value::Array(items)
    }

    fn build_str(s: &str) -> Value {
        Value::Utf8String(s.to_string())
    }

    #[test]
    fn test_parse_and_evaluate_dirname() {
        let ast = build_array(vec![build_str("dirname"), build_str(".git")]);
        let compiled = parse_expr(&ast);
        assert!(compiled.evaluate(".git"));
        assert!(compiled.evaluate(".git/config"));
        assert!(compiled.evaluate("foo/.git/config")); // from contain check
        assert!(!compiled.evaluate("git_file.txt")); // no match
    }

    #[test]
    fn test_parse_and_evaluate_name() {
        let ast = build_array(vec![
            build_str("name"),
            build_array(vec![build_str(".jj")]),
            build_str("wholename"),
        ]);
        let compiled = parse_expr(&ast);
        assert!(compiled.evaluate(".jj"));
        assert!(compiled.evaluate(".jj/repo"));
        assert!(!compiled.evaluate("foo/.jj"));

        let ast_base = build_array(vec![
            build_str("name"),
            build_array(vec![build_str("foo.txt"), build_str("bar.txt")]),
        ]);
        let compiled_base = parse_expr(&ast_base);
        assert!(compiled_base.evaluate("foo.txt"));
        assert!(compiled_base.evaluate("dir/foo.txt"));
        assert!(compiled_base.evaluate("bar.txt"));
        assert!(!compiled_base.evaluate("foo.txt.bak"));
    }

    #[test]
    fn test_parse_and_evaluate_complex() {
        // ["not", ["anyof", ["dirname", ".git"], ["dirname", ".jj"]]]
        let ast = build_array(vec![
            build_str("not"),
            build_array(vec![
                build_str("anyof"),
                build_array(vec![build_str("dirname"), build_str(".git")]),
                build_array(vec![build_str("dirname"), build_str(".jj")]),
            ]),
        ]);
        let compiled = parse_expr(&ast);
        assert!(!compiled.evaluate(".git/config"));
        assert!(!compiled.evaluate(".jj/state"));
        assert!(compiled.evaluate("src/main.rs"));
    }

    #[test]
    fn test_unsupported_expressions_fallback_to_true() {
        // e.g. ["future_op", "args"]
        let ast = build_array(vec![build_str("future_op"), build_str("some_arg")]);
        let compiled = parse_expr(&ast);
        assert!(compiled.evaluate("any/random/file.txt"));
        assert!(compiled.evaluate(".jj"));

        // Malformed `not` without arguments
        let ast_malformed_not = build_array(vec![build_str("not")]);
        let compiled_not = parse_expr(&ast_malformed_not);
        assert!(compiled_not.evaluate("file.txt")); // defaults to True

        // Not even an array
        let ast_string = build_str("just a string");
        let compiled_string = parse_expr(&ast_string);
        assert!(compiled_string.evaluate("file.txt"));
    }
}
