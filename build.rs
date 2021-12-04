use std::path::PathBuf;

fn main() {
    let langs = [
        "tree-sitter-json",
        "tree-sitter-python",
        "tree-sitter-rust",
        "tree-sitter-java",
    ];

    for name in langs {
        let dir: PathBuf = ["languages", name, "src"].iter().collect();

        let parser = dir.join("parser.c");
        let scanner = dir.join("scanner.c");
        let scanner_cpp = dir.join("scanner.cc");

        let mut builder = cc::Build::new();
        builder.include(&dir);
        if scanner.exists() {
            builder.file(scanner);
        }
        builder.file(parser).compile(name);

        if scanner_cpp.exists() {
            let mut builder = cc::Build::new();
            builder.include(&dir).cpp(true).file(scanner_cpp);
            builder.compile(&format!("{}_cpp", name));
        }
    }
}
