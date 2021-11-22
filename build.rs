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
        let builder = builder.include(&dir);
        builder.file(parser);

        if scanner.exists() {
            builder.file(scanner);
        }
        if scanner_cpp.exists() {
            builder.file(scanner_cpp);
            builder.cpp(true);
        }

        builder.compile(name);
    }
}
