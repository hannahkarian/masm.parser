use lalrpop_util::lalrpop_mod; // import the macro that loads the parser LALRPOP generated

mod ast; // include src/ast.rs (the data types) in this program
mod verilog; // include src/verilog.rs (checks + Verilog printer)
lalrpop_mod!(masm); // load the parser generated from masm.lalrpop as a module named `masm`

const DEFAULT_OUTPUT: &str = "generated.sv"; // file we write the Verilog into if no second argument is given

#[cfg(test)]
mod tests; // src/tests.rs, only compiled for `cargo test`

fn main() { // the program starts here
    let path = std::env::args() // get the command-line arguments (0 = program name, 1 = first argument)
        .nth(1) // take argument 1, the .masm file; gives Some(path) or None if missing
        .expect("usage: masm_parser <file.masm> [output.sv]"); // if None, stop and print this message
    let output = std::env::args().nth(2).unwrap_or(DEFAULT_OUTPUT.to_string()); // optional argument 2: output file
    let text = std::fs::read_to_string(&path).expect("could not read file"); // read the whole .masm file into a String

    // 1. MASM text -> AST
    let program = match masm::ProgramParser::new().parse(&text) { // create the parser and parse the text
        Ok(program) => program, // parsing worked: keep the Program (the AST)
        Err(e) => { // parsing failed: `e` describes the syntax error
            eprintln!("parse error: {}", e); // print the error to stderr
            std::process::exit(1); // stop the program with exit code 1 (= failure)
        }
    };

    // 2. check the program and collect the signal widths
    let mut ctx = match verilog::Context::new(&program, "Main", "clock") { // check the AST; module name "Main", clock port "clock"
        Ok(ctx) => ctx, // no errors: keep the Context (`mut` because we change ctx.source below)
        Err(errors) => { // one or more errors were found
            for error in errors { // go through every error message
                eprintln!("error: {}", error); // print it to stderr
            }
            std::process::exit(1); // stop with exit code 1
        }
    };
    for warning in &ctx.warnings { // go through the warnings (they don't stop the program)
        eprintln!("warning: {}", warning); // print each warning
    }
    ctx.source = Some(path.clone()); // store a copy of the file name, shown in the header of generated.sv

    // 3. AST -> Verilog -> generated.sv
    let verilog = verilog::render(&program, &ctx); // turn the AST into Verilog text (a String)
    std::fs::write(&output, verilog).expect("could not write the output file"); // write the text to the output file
    println!("wrote {}", output); // tell the user it worked
}