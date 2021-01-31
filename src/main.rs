use anyhow::Result;
use ehframe_bpf_compiler::EhFrame;

const BIN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/debug/examples/hello_world"
);

fn main() -> Result<()> {
    let frame = EhFrame::parse(BIN_PATH)?;
    /*println!("{} unwind tables", frame.tables.len());
    for table in &frame.tables {
        println!("{}", table);
    }*/

    let mut eh_elf = std::fs::File::create("eh_elf.c")?;
    ehframe_bpf_compiler::gen(&mut eh_elf, &frame)?;
    Ok(())
}
