use lalrpop_util::lalrpop_mod;

mod ast;
mod verilog;
lalrpop_mod!(masm);

#[cfg(test)]
mod tests;

const DEFAULT_OUTPUT: &str = "generated.sv";

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: masm_parser <file.masm> [output.sv]");
    let output = std::env::args()
        .nth(2)
        .unwrap_or(DEFAULT_OUTPUT.to_string());
    let text = std::fs::read_to_string(&path).expect("could not read file");

    let program = match masm::ProgramParser::new().parse(&text) {
        Ok(program) => program,
        Err(e) => {
            eprintln!("parse error: {}", e);
            std::process::exit(1);
        }
    };

    let mut ctx = match verilog::Context::new(&program, "Main", "clock") {
        Ok(ctx) => ctx,
        Err(errors) => {
            for error in errors {
                eprintln!("error: {}", error);
            }
            std::process::exit(1);
        }
    };
    for warning in &ctx.warnings {
        eprintln!("warning: {}", warning);
    }
    ctx.source = Some(path.clone());

    let verilog = verilog::render(&program, &ctx);
    std::fs::write(&output, verilog).expect("could not write the output file");
    println!("wrote {}", output);
}
