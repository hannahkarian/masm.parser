use lalrpop_util::lalrpop_mod;

mod ast;
lalrpop_mod!(masm);

fn main() {
    // reads finish.masm into a string
    let path = std::env::args() // gives all command line arguments
        .nth(1) // argument 0 is the program, argument 1 is finish.masm. takes argument 1 and returns Option<String>
        .expect("usage: masm_parser <file.masm>");
    let text = std::fs::read_to_string(&path).expect("could not read file");

    match masm::ProgramParser::new().parse(&text) {
        // print the program from ast.rs
        Ok(program) => println!("{:#?}", program),
        Err(e) => eprintln!("parse error: {}", e),
    }
}
