// We use rust-skeptic to test documentation in markdown files. *keeps things fresh*
extern crate skeptic;

fn main() {
    // generates doc tests for `README.md`.
    skeptic::generate_doc_tests(&["README.md"]);
}
