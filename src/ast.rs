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

impl Register {
    pub fn is_implemented(&self) -> bool {
        *self != Self::Unimplemented
    }

    pub fn is_defined(&self) -> bool {
        self.is_implemented() && *self != Self::Undefined
    }
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
    Rsp,
    Rbp,
    //Rbx,
    Ra,
}

impl From<gimli::Register> for MachineRegister {
    fn from(reg: gimli::Register) -> Self {
        match reg {
            gimli::X86_64::RSP => Self::Rsp,
            gimli::X86_64::RBP => Self::Rbp,
            //gimli::X86_64::RBX => Self::Rbx,
            gimli::X86_64::RA => Self::Ra,
            _ => todo!(),
        }
    }
}

impl std::fmt::Display for MachineRegister {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use MachineRegister::*;
        match self {
            Rsp => write!(f, "rsp"),
            Rbp => write!(f, "rbp"),
            //Rbx => write!(f, "rbx"),
            Ra => write!(f, "ra"),
        }
    }
}

/// Row of a FDE.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnwindTableRow {
    /// Instruction pointer start range (inclusive).
    pub start_address: usize,
    /// Instruction pointer end range (exclusive).
    pub end_address: usize,
    /// Canonical frame address.
    pub cfa: Register,
    /// Base pointer register.
    pub rbp: Register,
    // /// RBX, sometimes used for unwinding.
    // pub rbx: Register,
    /// Return address.
    pub ra: Register,
}

impl UnwindTableRow {
    pub fn parse<R: Reader>(
        row: &gimli::UnwindTableRow<R>,
        _encoding: gimli::Encoding,
    ) -> Result<Self> {
        Ok(Self {
            start_address: row.start_address() as _,
            end_address: row.end_address() as _,
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
            /*rbx: match row.register(gimli::X86_64::RBX) {
                RegisterRule::Undefined => Register::Undefined,
                RegisterRule::Offset(offset) => Register::CfaOffset(offset as _),
                _ => Register::Unimplemented,
            },*/
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
            "0x{:<6x}-0x{:<6x} {:8} {:8} {:8}",
            self.start_address,
            self.end_address,
            self.cfa.to_string(),
            self.rbp.to_string(),
            //self.rbx.to_string(),
            self.ra.to_string()
        )
    }
}

/// Unwind table.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UnwindTable {
    pub rows: Vec<UnwindTableRow>,
}

impl UnwindTable {
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
        let mut rows = vec![];
        while let Some(entry) = entries.next()? {
            match entry {
                gimli::CieOrFde::Cie(_) => {}
                gimli::CieOrFde::Fde(partial) => {
                    let fde = partial.parse(|_, bases, o| eh_frame.cie_from_offset(bases, o))?;
                    let encoding = fde.cie().encoding();
                    let mut table = fde.rows(&eh_frame, &bases, &mut ctx)?;
                    while let Some(row) = table.next_row()? {
                        rows.push(UnwindTableRow::parse(row, encoding)?);
                    }
                }
            }
        }
        rows.sort_unstable_by_key(|row| row.start_address);
        Ok(Self { rows })
    }
}

impl std::fmt::Display for UnwindTable {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(
            f,
            "{:18} {:8} {:8} {:8}",
            "ip", "cfa", "rbp", "ra"
        )?;
        for row in &self.rows {
            writeln!(f, "{}", row)?;
        }
        Ok(())
    }
}
