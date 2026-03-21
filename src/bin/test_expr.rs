use serde_json::json;
use watchman_client::expr;
fn main() {
    let exclude_dirs = vec![".git", ".jj"];
    let mut excludes: Vec<expr::Expr> = vec![
        expr::Expr::Name(expr::NameTerm {
            paths: exclude_dirs.iter().map(|&n| std::path::PathBuf::from(n)).collect(),
            wholename: true,
        }),
    ];
    for d in &exclude_dirs {
        excludes.push(expr::Expr::DirName(expr::DirNameTerm {
            path: std::path::PathBuf::from(*d),
            depth: None,
        }));
    }
    let e = expr::Expr::Not(Box::new(expr::Expr::Any(excludes)));
    println!("{}", serde_json::to_string(&e).unwrap());
}
