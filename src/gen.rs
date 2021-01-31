use crate::{EhFrame, Register};
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

unwind_context_t _eh_elf(unwind_context_t ctx, uintptr_t pc, deref_func_t deref) {
    unwind_context_t out_ctx;
    switch(pc) {
"#;

const POST: &str = r#"
        default:
            out_ctx.flags = 7; // UNWF_ERROR
            return out_ctx;
    }
}
"#;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Flags {
    rip: bool,
    rsp: bool,
    rbp: bool,
    rbx: bool,
    error: bool,
}

impl From<Flags> for u8 {
    fn from(flags: Flags) -> Self {
        ((flags.rip as u8) << 0) |
        ((flags.rsp as u8) << 1) |
        ((flags.rbp as u8) << 2) |
        ((flags.rbx as u8) << 3) |
        ((flags.error as u8) << 7)
    }
}

pub fn gen<W: Write>(mut w: W, eh_frame: &EhFrame) -> Result<()> {
    w.write_all(PRE.as_bytes())?;
    for table in &eh_frame.tables {
        let end = table.end_address;
        let mut iter = table.rows.iter().peekable();
        while let Some(row) = iter.next() {
            let start = row.ip;
            let end = iter.peek().map(|row| row.ip).unwrap_or(end) - 1;
            let mut flags = Flags::default();
            if !row.ra.is_implemented() {
                // RA might be undefined (last frame), but if it is defined and we
                // don't implement it (eg. EXPR), it is an error.
                flags.error = true;
            }
            let rsp = if row.cfa.is_implemented() {
                flags.rsp = true;
                format!("out_ctx.rsp = {};\n", gen_of_reg(row.cfa))
            } else {
                // rsp is required (CFA)
                flags.error = true;
                Default::default()
            };
            let rbp = if row.rbp.is_defined() {
                flags.rbp = true;
                format!("out_ctx.rbp = {};\n", gen_of_reg(row.rbp))
            } else {
                Default::default()
            };
            let ra = if row.ra.is_defined() {
                flags.rip = true;
                format!("out_ctx.rip = {};\n", gen_of_reg(row.ra))
            } else {
                Default::default()
            };
            let rbx = if row.rbx.is_defined() {
                flags.rbx = true;
                format!("out_ctx.rbx = {};\n", gen_of_reg(row.rbx))
            } else {
                Default::default()
            };

            let case = format!(r#"
        case 0x{:x} ... 0x{:x}:
               {}{}{}{}
               out_ctx.flags = {}u;
               return out_ctx;
            "#, start, end, rsp, rbp, ra, rbx, u8::from(flags));
            w.write_all(case.as_bytes())?;
        }
    }
    w.write_all(POST.as_bytes())?;
    Ok(())
}

fn gen_of_reg(reg: Register) -> String {
    match reg {
        Register::Undefined => unreachable!(),
        Register::Register(reg, offset) => {
            format!("ctx.{} + {}", reg, offset)
        }
        Register::CfaOffset(offset) => {
            format!("deref(out_ctx.rsp + {})", offset)
        }
        Register::PltExpr => "(((ctx.rip & 15) >= 11) ? 8 : 0) + ctx.rsp".to_string(),
        Register::Unimplemented => unreachable!(),
    }
}
