use anyhow::Result;
use ehframe_bpf_compiler::EhFrame;
use std::io::Write;

const BIN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/debug/examples/hello_world"
);

fn main() -> Result<()> {
    let frame = EhFrame::parse(BIN_PATH)?;
    let mut eh_tables = std::fs::File::create("eh_elf.txt")?;
    writeln!(&mut eh_tables, "{} unwind tables", frame.tables.len())?;
    for table in &frame.tables {
        writeln!(&mut eh_tables, "{}", table)?;
    }

    let mut eh_elf = std::fs::File::create("eh_elf.c")?;
    ehframe_bpf_compiler::gen(&mut eh_elf, &frame)?;
    Ok(())
}
