use crate::{UnwindTable, UnwindTableRow, Register};
use anyhow::Result;
use std::io::Write;

const PRE: &str = r#"
#include <assert.h>
#include <stdint.h>

typedef enum {
    UNWF_RIP=0,
    UNWF_RSP=1,
    UNWF_RBP=2,
    UNWF_RBX=3,
    UNWF_ERROR=7,
} unwind_flags_t;

typedef struct {
    uint8_t flags;
    uintptr_t rip, rsp, rbp, rbx;
} unwind_context_t;

typedef uintptr_t (*deref_func_t)(uintptr_t);

typedef unwind_context_t (*_fde_func_t)(unwind_context_t, uintptr_t);
typedef unwind_context_t (*_fde_func_with_deref_t)(
    unwind_context_t,
    uintptr_t,
    deref_func_t);

void _eh_elf(unwind_context_t ctx, unwind_context_t *out_ctx, uintptr_t pc, deref_func_t deref) {
"#;

const POST: &str = r#"
    out_ctx->flags = 7; // UNWF_ERROR
    return;
}
"#;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UnwindFlags {
    rip: bool,
    rsp: bool,
    rbp: bool,
    rbx: bool,
    error: bool,
}

impl From<UnwindFlags> for u8 {
    fn from(flags: UnwindFlags) -> Self {
        ((flags.rip as u8) << 0) |
        ((flags.rsp as u8) << 1) |
        ((flags.rbp as u8) << 2) |
        ((flags.rbx as u8) << 3) |
        ((flags.error as u8) << 7)
    }
}

impl UnwindTable {
    pub fn gen<W: Write>(&self, w: &mut W) -> Result<()> {
        w.write_all(PRE.as_bytes())?;
        gen_rows(w, &self.rows)?;
        w.write_all(POST.as_bytes())?;
        Ok(())
    }
}

fn gen_rows<W: Write>(w: &mut W, rows: &[UnwindTableRow]) -> Result<()> {
    if rows.len() > 1 {
        let (a, b) = rows.split_at(rows.len() / 2);
        writeln!(
            w,
            "if(0x{:x} <= pc && pc < 0x{:x}) {{",
            a.first().unwrap().start_address,
            a.last().unwrap().end_address,
        )?;
        gen_rows(w, a)?;
        writeln!(w, "}} else {{")?;
        gen_rows(w, b)?;
        writeln!(w, "}}")?;
    } else {
        rows[0].gen(w)?;
    }
    Ok(())
}

impl UnwindTableRow {
    pub fn gen<W: Write>(&self, w: &mut W) -> Result<()> {
        let mut flags = UnwindFlags::default();
        if !self.ra.is_implemented() {
            // RA might be undefined (last frame), but if it is defined and we
            // don't implement it (eg. EXPR), it is an error.
            flags.error = true;
        }
        if self.cfa.is_implemented() {
            flags.rsp = true;
            write!(w, "out_ctx->rsp = ")?;
            self.cfa.gen(w)?;
            write!(w, ";\n")?;
        } else {
            // rsp is required (CFA)
            flags.error = true;
        }
        if self.rbp.is_defined() {
            flags.rbp = true;
            write!(w, "out_ctx->rbp = ")?;
            self.rbp.gen(w)?;
            write!(w, ";\n")?;
        }
        if self.ra.is_defined() {
            flags.rip = true;
            write!(w, "out_ctx->rip = ")?;
            self.ra.gen(w)?;
            write!(w, ";\n")?;
        }
        /*if row.rbx.is_defined() {
            flags.rbx = true;
            writeln!(w, "out_ctx->rbx = {};\n", gen_of_reg(row.rbx))?;
        }*/
        writeln!(w, "out_ctx->flags = {}u;", u8::from(flags))?;
        writeln!(w, "return;")?;
        Ok(())
    }
}

impl Register {
    pub fn gen<W: Write>(&self, w: &mut W) -> Result<()> {
        match self {
            Self::CfaOffset(offset) => {
                write!(w, "deref(out_ctx->rsp + {})", offset)?
            }
            Self::Register(reg, offset) => {
                write!(w, "ctx.{} + {}", reg, offset)?
            }
            Self::PltExpr => write!(w, "(((ctx.rip & 15) >= 11) ? 8 : 0) + ctx.rsp")?,
            Self::Undefined => unreachable!(),
            Self::Unimplemented => unreachable!(),
        }
        Ok(())
    }
}
