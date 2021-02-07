use anyhow::Result;
use ehframe_bpf_compiler::UnwindTable;
use std::path::Path;
use std::process::Command;

fn main() -> Result<()> {
    let input = std::env::args().skip(1).next().expect("input binary");
    let mut output_so = Path::new(&input).file_name().unwrap().to_owned();
    let mut output_c = output_so.clone();
    output_c.push(".eh_elf.c");
    output_so.push(".eh_elf.so");

    let table = UnwindTable::parse(input)?;
    let mut eh_elf = std::fs::File::create(&output_c)?;
    table.gen(&mut eh_elf)?;

    let output = Command::new("clang")
        .arg(output_c)
        .arg("-shared")
        .arg("-o")
        .arg(output_so)
        .output()?;
    print!("{}", std::str::from_utf8(&output.stdout)?);
    eprint!("{}", std::str::from_utf8(&output.stderr)?);
    std::process::exit(output.status.code().unwrap());
}
