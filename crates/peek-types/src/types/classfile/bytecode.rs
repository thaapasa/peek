//! JVM bytecode disassembly (`javap -c` style).
//!
//! The Info / Fields / Methods views parse with `parse_bytecode(false)`
//! for speed; this re-parses with bytecode enabled and turns each
//! method's `Code` attribute into a flat instruction listing. The model
//! here is theme-free plain data — colouring and layout live in
//! [`super::bytecode_mode::BytecodeMode`].

use anyhow::{Result, anyhow};
use cafebabe::attributes::AttributeData;
use cafebabe::bytecode::Opcode;
use cafebabe::{MethodInfo, ParseOptions, parse_class_with_options};

use super::descriptor;
use crate::input::InputSource;

/// One disassembled class: its methods in declaration order.
pub struct Disassembly {
    pub methods: Vec<MethodAsm>,
}

/// One method: a header line plus its instruction stream. Methods with
/// no `Code` attribute (abstract / native) carry an empty instruction
/// list and a marker note instead.
pub struct MethodAsm {
    /// `name (int, String) -> void`.
    pub signature: String,
    /// `Some` for abstract / native methods — no bytecode to show.
    pub note: Option<&'static str>,
    pub instructions: Vec<Insn>,
}

/// One instruction: its byte offset, mnemonic, and rendered operand
/// (empty for operand-less opcodes).
pub struct Insn {
    pub offset: usize,
    pub mnemonic: String,
    pub operand: String,
}

/// Parse `source` with bytecode enabled and disassemble every method.
pub fn build(source: &InputSource) -> Result<Disassembly> {
    let bytes = source.read_bytes()?;
    let mut opts = ParseOptions::default();
    opts.parse_bytecode(true);
    let class = parse_class_with_options(&bytes, &opts)
        .map_err(|e| anyhow!("not a valid classfile: {e}"))?;
    Ok(Disassembly {
        methods: class.methods.iter().map(method_asm).collect(),
    })
}

fn method_asm(method: &MethodInfo<'_>) -> MethodAsm {
    let signature = format!("{} {}", method.name, signature_text(method));
    match method_bytecode(method) {
        Some(opcodes) => MethodAsm {
            signature,
            note: None,
            instructions: opcodes
                .iter()
                .map(|(offset, op)| Insn {
                    offset: *offset,
                    mnemonic: mnemonic(op),
                    operand: operand(op, *offset),
                })
                .collect(),
        },
        None => MethodAsm {
            signature,
            note: Some("(no code — abstract or native)"),
            instructions: Vec::new(),
        },
    }
}

/// Plain-text method signature, reusing the table descriptor formatter:
/// `(int, String) -> void`.
fn signature_text(method: &MethodInfo<'_>) -> String {
    descriptor::method(&method.descriptor)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

/// The decoded opcode stream from a method's `Code` attribute.
fn method_bytecode<'m, 'a>(method: &'m MethodInfo<'a>) -> Option<&'m [(usize, Opcode<'a>)]> {
    method.attributes.iter().find_map(|attr| match &attr.data {
        AttributeData::Code(code) => code.bytecode.as_ref().map(|bc| bc.opcodes.as_slice()),
        _ => None,
    })
}

/// Lowercased mnemonic. cafebabe has no `Display`, and its CamelCase
/// variant names drop the underscores the canonical JVM mnemonics carry
/// (`Iconst0` vs `iconst_0`, `IfIcmpeq` vs `if_icmpeq`). The composite
/// mnemonics are mapped explicitly; everything else lowercases its
/// `Debug` variant name (the part before the operand `(`).
fn mnemonic(op: &Opcode<'_>) -> String {
    if let Some(canonical) = canonical_mnemonic(op) {
        return canonical.to_string();
    }
    let dbg = format!("{op:?}");
    dbg.split(['(', ' '])
        .next()
        .unwrap_or(&dbg)
        .to_ascii_lowercase()
}

/// Canonical spelling for the opcodes whose JVM mnemonic has an
/// underscore the cafebabe variant name lacks. `None` for everything
/// else (the lowercased variant name is already correct).
fn canonical_mnemonic(op: &Opcode<'_>) -> Option<&'static str> {
    use Opcode::*;
    Some(match op {
        AconstNull => "aconst_null",
        IconstM1 => "iconst_m1",
        Iconst0 => "iconst_0",
        Iconst1 => "iconst_1",
        Iconst2 => "iconst_2",
        Iconst3 => "iconst_3",
        Iconst4 => "iconst_4",
        Iconst5 => "iconst_5",
        Lconst0 => "lconst_0",
        Lconst1 => "lconst_1",
        Fconst0 => "fconst_0",
        Fconst1 => "fconst_1",
        Fconst2 => "fconst_2",
        Dconst0 => "dconst_0",
        Dconst1 => "dconst_1",
        DupX1 => "dup_x1",
        DupX2 => "dup_x2",
        Dup2X1 => "dup2_x1",
        Dup2X2 => "dup2_x2",
        LdcW(_) => "ldc_w",
        Ldc2W(_) => "ldc2_w",
        IfIcmpeq(_) => "if_icmpeq",
        IfIcmpne(_) => "if_icmpne",
        IfIcmplt(_) => "if_icmplt",
        IfIcmpge(_) => "if_icmpge",
        IfIcmpgt(_) => "if_icmpgt",
        IfIcmple(_) => "if_icmple",
        IfAcmpeq(_) => "if_acmpeq",
        IfAcmpne(_) => "if_acmpne",
        _ => return None,
    })
}

/// Rendered operand for the opcodes that carry one; empty otherwise.
/// Branch targets are resolved to absolute byte offsets.
fn operand(op: &Opcode<'_>, offset: usize) -> String {
    use Opcode::*;
    match op {
        Aload(i) | Astore(i) | Iload(i) | Istore(i) | Lload(i) | Lstore(i) | Fload(i)
        | Fstore(i) | Dload(i) | Dstore(i) | Ret(i) => i.to_string(),
        Bipush(v) => v.to_string(),
        Sipush(v) => v.to_string(),
        Iinc(index, amount) => format!("{index} by {amount}"),
        Getfield(m) | Getstatic(m) | Putfield(m) | Putstatic(m) | Invokespecial(m)
        | Invokestatic(m) | Invokevirtual(m) => member_ref(m),
        Invokeinterface(m, _) => member_ref(m),
        Invokedynamic(d) => format!("{d:?}"),
        New(class) => class.to_string(),
        Newarray(t) => format!("{t:?}").to_ascii_lowercase(),
        Anewarray(t) | Checkcast(t) | Instanceof(t) => format!("{t:?}"),
        Multianewarray(t, dims) => format!("{t:?} dims={dims}"),
        Ldc(l) | LdcW(l) | Ldc2W(l) => loadable(l),
        Tableswitch(t) => format!("{} entries", t.jumps.len()),
        Lookupswitch(t) => format!("{} entries", t.match_offsets.len()),
        Goto(j) | Jsr(j) | IfAcmpeq(j) | IfAcmpne(j) | IfIcmpeq(j) | IfIcmpge(j) | IfIcmpgt(j)
        | IfIcmple(j) | IfIcmplt(j) | IfIcmpne(j) | Ifeq(j) | Ifge(j) | Ifgt(j) | Ifle(j)
        | Iflt(j) | Ifne(j) | Ifnonnull(j) | Ifnull(j) => {
            // Branch offsets are relative to the branch instruction;
            // show the absolute target so it lines up with an offset
            // column.
            format!("{}", offset as i64 + *j as i64)
        }
        _ => String::new(),
    }
}

/// `class.name:descriptor` for a field / method reference.
fn member_ref(m: &cafebabe::constant_pool::MemberRef<'_>) -> String {
    format!(
        "{}.{}:{}",
        m.class_name, m.name_and_type.name, m.name_and_type.descriptor
    )
}

/// A loadable constant-pool entry (`ldc` operand). Class / method-type
/// names render plainly; literals fall back to their `Debug` form.
fn loadable(l: &cafebabe::constant_pool::Loadable<'_>) -> String {
    use cafebabe::constant_pool::Loadable::*;
    match l {
        ClassInfo(name) | MethodType(name) => name.to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample() -> InputSource {
        let mut p = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        p.push("test-data/Sample.class");
        InputSource::File(p)
    }

    fn all_insns(disasm: &Disassembly) -> Vec<&Insn> {
        disasm
            .methods
            .iter()
            .flat_map(|m| &m.instructions)
            .collect()
    }

    /// The constructor's `invokespecial Object.<init>` is disassembled
    /// with a resolved member reference (cf. `javap -c`).
    #[test]
    fn resolves_invokespecial_member_ref() {
        let disasm = build(&sample()).unwrap();
        let insns = all_insns(&disasm);
        assert!(
            insns.iter().any(|i| i.mnemonic == "invokespecial"
                && i.operand.contains("java/lang/Object")
                && i.operand.contains("<init>")),
            "invokespecial Object.<init> is present"
        );
    }

    /// Local-variable loads keep their index operand, and at least one
    /// method ends in `return`.
    #[test]
    fn renders_indexed_loads_and_return() {
        let disasm = build(&sample()).unwrap();
        let insns = all_insns(&disasm);
        assert!(
            insns
                .iter()
                .any(|i| i.mnemonic == "aload" && i.operand == "0"),
            "aload_0 renders as `aload 0`"
        );
        assert!(
            insns.iter().any(|i| i.mnemonic == "return"),
            "a void method returns"
        );
    }

    /// Composite mnemonics keep their canonical underscores; plain ones
    /// fall through to the lowercased variant name.
    #[test]
    fn canonical_mnemonics_keep_underscores() {
        use cafebabe::bytecode::Opcode;
        assert_eq!(mnemonic(&Opcode::Iconst0), "iconst_0");
        assert_eq!(mnemonic(&Opcode::IconstM1), "iconst_m1");
        assert_eq!(mnemonic(&Opcode::AconstNull), "aconst_null");
        assert_eq!(mnemonic(&Opcode::DupX1), "dup_x1");
        assert_eq!(mnemonic(&Opcode::Dup2X1), "dup2_x1");
        assert_eq!(mnemonic(&Opcode::IfIcmpeq(0)), "if_icmpeq");
        assert_eq!(mnemonic(&Opcode::IfAcmpne(0)), "if_acmpne");
        // No underscore — the Debug-derived fallback is already correct.
        assert_eq!(mnemonic(&Opcode::Iadd), "iadd");
        assert_eq!(mnemonic(&Opcode::Areturn), "areturn");
    }

    /// Every method carries a signature header.
    #[test]
    fn methods_have_signatures() {
        let disasm = build(&sample()).unwrap();
        assert!(!disasm.methods.is_empty());
        assert!(disasm.methods.iter().all(|m| !m.signature.is_empty()));
    }
}
