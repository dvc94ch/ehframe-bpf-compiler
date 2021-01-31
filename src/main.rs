use anyhow::Result;
use ehframe_bpf_compiler::UnwindTable;
use std::io::Write;

const BIN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/debug/examples/hello_world"
);

fn main() -> Result<()> {
    let table = UnwindTable::parse(BIN_PATH)?;
    let mut eh_table = std::fs::File::create("eh_elf.txt")?;
    writeln!(&mut eh_table, "{}", table)?;

    let mut eh_elf = std::fs::File::create("eh_elf.c")?;
    table.gen(&mut eh_elf)?;
    Ok(())
}
