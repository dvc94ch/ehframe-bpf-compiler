use anyhow::Result;
use gimli::{
    CfaRule, NativeEndian, Reader, RegisterRule, UninitializedUnwindContext, UnwindSection,
};
use object::{Object, ObjectSection};
use std::path::Path;

/// Holds a single dwarf register value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Register {
    /// Undefined register. The value will be defined at some
    /// later IP in the same DIE.
    Undefined,
    /// Value of a machine register plus offset.
    Register(MachineRegister, isize),
    /// Value stored at some offset from `CFA`.
    CfaOffset(isize),
    /// Value is the evaluation of the standard PLT
    /// expression, ie `((rip & 15) >= 11) >> 3 + rsp`.
    /// This is hardcoded because it is a common expression.
    PltExpr,
    /// This type of register is not supported.
    Unimplemented,
}

impl std::fmt::Display for Register {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Undefined => write!(f, "undef"),
            Self::Register(mreg, offset) => {
                let op = if *offset >= 0 { "+" } else { "" };
                write!(f, "{}{}{}", mreg, op, offset)
            }
            Self::CfaOffset(offset) => {
                let op = if *offset >= 0 { "+" } else { "" };
                write!(f, "cfa{}{}", op, offset)
            }
            Self::PltExpr => write!(f, "plt"),
            Self::Unimplemented => write!(f, "unimpl"),
        }
    }
}

/// A machine register (eg. %rip) among the supported ones (x86_64 only for now).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MachineRegister {
    Rip,
    Rsp,
    Rbp,
    Rbx,
    // A bit of cheating: not a machine register.
    Ra,
}

impl From<gimli::Register> for MachineRegister {
    fn from(reg: gimli::Register) -> Self {
        match reg {
            gimli::X86_64::RSP => Self::Rsp,
            gimli::X86_64::RBP => Self::Rbp,
            gimli::X86_64::RBX => Self::Rbx,
            gimli::X86_64::RA => Self::Ra,
            _ => todo!(),
        }
    }
}

impl std::fmt::Display for MachineRegister {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use MachineRegister::*;
        match self {
            Rip => write!(f, "rip"),
            Rsp => write!(f, "rsp"),
            Rbp => write!(f, "rbp"),
            Rbx => write!(f, "rbx"),
            Ra => write!(f, "ra"),
        }
    }
}

/// Row of a FDE.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnwindTableRow {
    /// Instruction pointer.
    pub ip: usize,
    /// Canonical frame address.
    pub cfa: Register,
    /// Base pointer register.
    pub rbp: Register,
    /// RBX, sometimes used for unwinding.
    pub rbx: Register,
    /// Return address.
    pub ra: Register,
}

impl UnwindTableRow {
    pub fn parse<R: Reader>(
        row: &gimli::UnwindTableRow<R>,
        _encoding: gimli::Encoding,
    ) -> Result<Self> {
        Ok(Self {
            ip: row.start_address() as _,
            cfa: match row.cfa() {
                CfaRule::RegisterAndOffset { register, offset } => {
                    Register::Register((*register).into(), *offset as _)
                }
                CfaRule::Expression(_expr) => {
                    // TODO check it is always PltExpr
                    Register::PltExpr
                }
            },
            rbp: match row.register(gimli::X86_64::RBP) {
                RegisterRule::Undefined => Register::Undefined,
                RegisterRule::Offset(offset) => Register::CfaOffset(offset as _),
                _ => Register::Unimplemented,
            },
            rbx: match row.register(gimli::X86_64::RBX) {
                RegisterRule::Undefined => Register::Undefined,
                RegisterRule::Offset(offset) => Register::CfaOffset(offset as _),
                _ => Register::Unimplemented,
            },
            ra: match row.register(gimli::X86_64::RA) {
                RegisterRule::Undefined => Register::Undefined,
                RegisterRule::Offset(offset) => Register::CfaOffset(offset as _),
                _ => Register::Unimplemented,
            },
        })
    }
}

impl std::fmt::Display for UnwindTableRow {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "0x{:<6x} {:8} {:8} {:8} {:8}",
            self.ip,
            self.cfa.to_string(),
            self.rbp.to_string(),
            self.rbx.to_string(),
            self.ra.to_string()
        )
    }
}

/// Frame description entry.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnwindTable {
    /// This FDE's start instruction pointer incl.
    pub start_address: usize,
    /// This FDE's end instruction pointer excl.
    pub end_address: usize,
    /// Dwarf rows for this FDE.
    pub rows: Vec<UnwindTableRow>,
}

impl std::fmt::Display for UnwindTable {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "0x{:x}-0x{:x}", self.start_address, self.end_address)?;
        writeln!(
            f,
            "{:8} {:8} {:8} {:8} {:8}",
            "ip", "cfa", "rbp", "rbx", "ra"
        )?;
        for row in &self.rows {
            writeln!(f, "{}", row)?;
        }
        Ok(())
    }
}

/// EhFrame
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EhFrame {
    pub tables: Vec<UnwindTable>,
}

impl EhFrame {
    pub fn parse<T: AsRef<Path>>(path: T) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let file = unsafe { memmap::Mmap::map(&file) }?;
        let file = object::File::parse(&*file)?;

        let section = file.section_by_name(".eh_frame").unwrap();
        let data = section.uncompressed_data()?;
        let mut eh_frame = gimli::EhFrame::new(&data, NativeEndian);
        eh_frame.set_address_size(std::mem::size_of::<usize>() as _);

        let mut bases = gimli::BaseAddresses::default();
        if let Some(section) = file.section_by_name(".eh_frame_hdr") {
            bases = bases.set_eh_frame_hdr(section.address());
        }
        if let Some(section) = file.section_by_name(".eh_frame") {
            bases = bases.set_eh_frame(section.address());
        }
        if let Some(section) = file.section_by_name(".text") {
            bases = bases.set_text(section.address());
        }
        if let Some(section) = file.section_by_name(".got") {
            bases = bases.set_got(section.address());
        }

        let mut ctx = UninitializedUnwindContext::new();
        let mut entries = eh_frame.entries(&bases);
        let mut tables = vec![];
        while let Some(entry) = entries.next()? {
            match entry {
                gimli::CieOrFde::Cie(_) => {}
                gimli::CieOrFde::Fde(partial) => {
                    let fde = partial.parse(|_, bases, o| eh_frame.cie_from_offset(bases, o))?;
                    let encoding = fde.cie().encoding();
                    let mut table = fde.rows(&eh_frame, &bases, &mut ctx)?;
                    let mut rows = vec![];
                    let mut start_address = None;
                    let mut end_address = None;
                    while let Some(row) = table.next_row()? {
                        if start_address.is_none() {
                            start_address = Some(row.start_address());
                        }
                        end_address = Some(row.end_address());
                        rows.push(UnwindTableRow::parse(row, encoding)?);
                    }
                    if let (Some(start_address), Some(end_address)) = (start_address, end_address) {
                        tables.push(UnwindTable {
                            start_address: start_address as _,
                            end_address: end_address as _,
                            rows,
                        });
                    }
                }
            }
        }
        Ok(Self { tables })
    }
}