use crate::ir::{Instruction, Program};
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;

mod runtime;

pub const TARGET_SUPPORTED: bool =
    cfg!(all(target_os = "linux", target_arch = "x86_64"));

const PUSH_RBX: [u8; 1] = [0x53];
const PUSH_R12: [u8; 2] = [0x41, 0x54];
const PUSH_R13: [u8; 2] = [0x41, 0x55];
const PUSH_R14: [u8; 2] = [0x41, 0x56];

const POP_R14: [u8; 2] = [0x41, 0x5e];
const POP_R13: [u8; 2] = [0x41, 0x5d];
const POP_R12: [u8; 2] = [0x41, 0x5c];
const POP_RBX: [u8; 1] = [0x5b];

const MOV_RDI_TO_RBX: [u8; 3] = [0x48, 0x89, 0xfb];
const MOV_RSI_TO_R12: [u8; 3] = [0x49, 0x89, 0xf4];
const MOV_RDX_TO_R13: [u8; 3] = [0x49, 0x89, 0xd5];
const MOV_R12_TO_RDI: [u8; 3] = [0x4c, 0x89, 0xe7];
const MOV_R13_TO_RSI: [u8; 3] = [0x4c, 0x89, 0xee];
const MOV_RAX_TO_R12: [u8; 3] = [0x49, 0x89, 0xc4];
const MOV_RBX_TO_RDI: [u8; 3] = [0x48, 0x89, 0xdf];
const MOV_AX_TO_SI: [u8; 3] = [0x66, 0x89, 0xc9];
const MOV_AX_TO_MEM_R12_R14: [u8; 5] = [0x66, 0x43, 0x89, 0x04, 0x34];
const MOV_MEM_R12_R14_TO_AL: [u8; 4] = [0x43, 0x8a, 0x04, 0x34];
const MOVABS_TO_RAX: [u8; 2] = [0x48, 0xb8];

const CMP_R14_WITH_R13: [u8; 3] = [0x4d, 0x39, 0xee];
const TEST_R14_WITH_R14: [u8; 3] = [0x4d, 0x85, 0xf6];
const TEST_AX_WITH_AX: [u8; 3] = [0x66, 0x85, 0xc0];
const TEST_AL_WITH_AL: [u8; 2] = [0x84, 0xc0];

const JMP_REL32: [u8; 1] = [0xe9];
const JE_JZ_REL32: [u8; 2] = [0x0f, 0x84];
const JNE_JNZ_REL32: [u8; 2] = [0x0f, 0x85];
const JS_REL32: [u8; 2] = [0x0f, 0x88];
const CALL_ABS_RAX: [u8; 1] = [0xff];

const XOR_R14_TO_R14: [u8; 3] = [0x4d, 0x31, 0xf6];
const XOR_EAX_TO_EAX: [u8; 2] = [0x31, 0xc0];

const ADD_IMM32_TO_R13: [u8; 3] = [0x49, 0x81, 0xc5];
const ADD_IMM32_TO_R14: [u8; 3] = [0x49, 0x81, 0xc6];

const ROR_IMM8_TO_AX: [u8; 3] = [0x66, 0xc1, 0xc8];

const INC_R14: [u8; 3] = [0x49, 0xff, 0xc6];
const DEC_R14: [u8; 3] = [0x49, 0xff, 0xce];

const INCB_MEM_R12_R14: [u8; 4] = [0x43, 0xfe, 0x04, 0x34];
const DECB_MEM_R12_R14: [u8; 4] = [0x43, 0xfe, 0x0c, 0x34];

const RET: [u8; 1] = [0xc3];

#[derive(Debug, Error)]
pub enum Error {
    #[error("target is unsupported for Just-In-Time compilation")]
    UnsupportedTarget,
    #[error("label index {} is out of bounds", .0)]
    BadLabelIndex(usize),
    #[error("could not allocate memory for just in time compilation")]
    AllocError,
}

pub struct Executable;

pub fn compile(program: &Program) -> Result<Executable, Error> {
    if !TARGET_SUPPORTED {
        Err(Error::UnsupportedTarget)?;
    }

    let mut compiler = Compiler::new();

    compiler.first_pass(program);
    compiler.second_pass()?;

    todo!()
}

fn write_absolute_call(
    buf: &mut Vec<u8>,
    func_ptr: *const u8,
) -> Result<(), Error> {
    buf.extend(MOVABS_TO_RAX);
    buf.extend((func_ptr as usize as u64).to_le_bytes());
    buf.extend(CALL_ABS_RAX);
    Ok(())
}

#[derive(Debug, Clone)]
struct Compiler {
    buf: Vec<u8>,
    placeholders: BTreeMap<usize, (usize, usize)>,
    labels: HashMap<(usize, usize), usize>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            placeholders: BTreeMap::new(),
            labels: HashMap::new(),
        }
    }

    pub fn first_pass(&mut self, program: &Program) {
        let last_ir_label = program.code.len();
        for (ir_label, instr) in program.code.iter().enumerate() {
            self.def_main_label(ir_label);
            self.handle_instruction(ir_label, *instr, last_ir_label);
        }
        self.def_main_label(last_ir_label);
    }

    pub fn second_pass(&mut self) -> Result<(), Error> {
        for (placeholder_label, (ir_label, sub_ir_label)) in &self.placeholders
        {
            let Some(label) = self.labels.get(&(*ir_label, *sub_ir_label))
            else {
                Err(Error::BadLabelIndex(*ir_label))?
            };
            let label_buf = (*label as u32).to_le_bytes();
            self.buf[*placeholder_label .. *placeholder_label + 4]
                .copy_from_slice(&label_buf[..]);
        }
        Ok(())
    }

    pub fn handle_instruction(
        &mut self,
        ir_label: usize,
        instr: Instruction,
        last_ir_label: usize,
    ) {
        match instr {
            Instruction::Inc => self.write_inc(),
            Instruction::Dec => self.write_dec(),
            Instruction::Next => self.write_next(ir_label),
            Instruction::Prev => self.write_prev(ir_label),
            Instruction::Get => self.write_get(ir_label, last_ir_label),
            Instruction::Put => self.write_put(last_ir_label),
            Instruction::Jz(target_ir_label) => self.write_jz(target_ir_label),
            Instruction::Jnz(target_ir_label) => {
                self.write_jnz(target_ir_label)
            },
            Instruction::Halt => self.write_halt(last_ir_label),
        }
    }

    pub fn def_main_label(&mut self, ir_label: usize) {
        self.def_label(ir_label, 0)
    }

    pub fn def_label(&mut self, ir_label: usize, sub_label: usize) {
        self.labels.insert((ir_label, sub_label), self.buf.len());
    }

    pub fn make_placeholder(&mut self, ir_label: usize, sub_label: usize) {
        self.placeholders.insert(self.buf.len(), (ir_label, sub_label));
        self.buf.extend(0u32.to_le_bytes());
    }

    pub fn call_absolute(&mut self, func_ptr: *const u8) {
        self.buf.extend(MOVABS_TO_RAX);
        self.buf.extend((func_ptr as usize as u64).to_le_bytes());
        self.buf.extend(CALL_ABS_RAX);
    }

    pub fn write_enter(&mut self) {
        self.buf.extend(PUSH_RBX);
        self.buf.extend(PUSH_R12);
        self.buf.extend(PUSH_R13);
        self.buf.extend(PUSH_R14);
        self.buf.extend(MOV_RDI_TO_RBX);
        self.buf.extend(MOV_RSI_TO_R12);
        self.buf.extend(MOV_RDX_TO_R13);
        self.buf.extend(XOR_R14_TO_R14);
    }

    pub fn write_leave(&mut self, ir_label: usize) {
        self.buf.extend(XOR_EAX_TO_EAX);
        self.def_label(ir_label, 1);
        self.buf.extend(POP_R14);
        self.buf.extend(POP_R13);
        self.buf.extend(POP_R12);
        self.buf.extend(POP_RBX);
        self.buf.extend(RET);
    }

    pub fn write_inc(&mut self) {
        self.buf.extend(INCB_MEM_R12_R14);
    }

    pub fn write_dec(&mut self) {
        self.buf.extend(DECB_MEM_R12_R14);
    }

    pub fn write_next(&mut self, ir_label: usize) {
        self.buf.extend(CMP_R14_WITH_R13);
        self.buf.extend(JNE_JNZ_REL32);
        self.make_placeholder(ir_label, 1);
        self.buf.extend(MOV_R12_TO_RDI);
        self.buf.extend(MOV_R13_TO_RSI);
        self.call_absolute(runtime::grow_next as *const u8);
        self.buf.extend(MOV_RAX_TO_R12);
        self.buf.extend(ADD_IMM32_TO_R13);
        self.buf.extend((runtime::TAPE_CHUNK_SIZE as u32).to_le_bytes());
        self.def_label(ir_label, 1);
        self.buf.extend(INC_R14);
    }

    pub fn write_prev(&mut self, ir_label: usize) {
        self.buf.extend(TEST_R14_WITH_R14);
        self.buf.extend(JNE_JNZ_REL32);
        self.make_placeholder(ir_label, 1);
        self.buf.extend(MOV_R12_TO_RDI);
        self.buf.extend(MOV_R13_TO_RSI);
        self.call_absolute(runtime::grow_prev as *const u8);
        self.buf.extend(ADD_IMM32_TO_R14);
        self.buf.extend((runtime::TAPE_CHUNK_SIZE as u32).to_le_bytes());
        self.buf.extend(MOV_RAX_TO_R12);
        self.buf.extend(ADD_IMM32_TO_R13);
        self.buf.extend((runtime::TAPE_CHUNK_SIZE as u32).to_le_bytes());
        self.def_label(ir_label, 1);
        self.buf.extend(DEC_R14);
    }

    pub fn write_put(&mut self, last_ir_label: usize) {
        self.buf.extend(MOV_RBX_TO_RDI);
        self.buf.extend(XOR_EAX_TO_EAX);
        self.buf.extend(MOV_MEM_R12_R14_TO_AL);
        self.buf.extend(MOV_AX_TO_SI);
        self.call_absolute(runtime::put as *const u8);
        self.buf.extend(TEST_AL_WITH_AL);
        self.buf.extend(JS_REL32);
        self.make_placeholder(last_ir_label, 1);
    }

    pub fn write_get(&mut self, ir_label: usize, last_ir_label: usize) {
        self.buf.extend(CMP_R14_WITH_R13);
        self.buf.extend(JNE_JNZ_REL32);
        self.make_placeholder(ir_label, 1);
        self.buf.extend(MOV_R12_TO_RDI);
        self.buf.extend(MOV_R13_TO_RSI);
        self.call_absolute(runtime::grow_next as *const u8);
        self.buf.extend(MOV_RAX_TO_R12);
        self.buf.extend(ADD_IMM32_TO_R13);
        self.buf.extend((runtime::TAPE_CHUNK_SIZE as u32).to_le_bytes());
        self.def_label(ir_label, 1);
        self.buf.extend(MOV_RBX_TO_RDI);
        self.call_absolute(runtime::get as *const u8);
        self.buf.extend(TEST_AX_WITH_AX);
        self.buf.extend(JS_REL32);
        self.make_placeholder(last_ir_label, 1);
        self.buf.extend(ROR_IMM8_TO_AX);
        self.buf.extend(8u8.to_le_bytes());
        self.buf.extend(MOV_AX_TO_MEM_R12_R14);
    }

    pub fn write_halt(&mut self, last_ir_label: usize) {
        self.buf.extend(JMP_REL32);
        self.make_placeholder(last_ir_label, 0);
    }

    pub fn write_jz(&mut self, target_ir_label: usize) {
        self.buf.extend(JE_JZ_REL32);
        self.make_placeholder(target_ir_label, 0);
    }

    pub fn write_jnz(&mut self, target_ir_label: usize) {
        self.buf.extend(JNE_JNZ_REL32);
        self.make_placeholder(target_ir_label, 0);
    }
}
