//! `corex-lsp`: Language Server Protocol server over stdio.

fn main() {
    std::process::exit(corex::lsp::run());
}
