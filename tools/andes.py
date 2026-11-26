# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

CUSTOM_0 = 0x0B
CUSTOM_1 = 0x2B
CUSTOM_2 = 0x5B
MASK32 = 0xFFFFFFFF


def field(word, hi, lo):
    return (word >> lo) & ((1 << (hi - lo + 1)) - 1)


def sext(value, width):
    sign = 1 << (width - 1)
    return (value & (sign - 1)) - (value & sign)


def gp_common(word):
    return (field(word, 31, 31) << 17) | (field(word, 19, 17) << 12) | (field(word, 16, 15) << 15)


def gp_load_byte_offset(word):
    return sext(gp_common(word) | (field(word, 30, 21) << 1) | (field(word, 20, 20) << 11) | field(word, 14, 14), 18)


def gp_store_byte_offset(word):
    return sext(gp_common(word) | (field(word, 30, 25) << 5) | (field(word, 11, 8) << 1) | (field(word, 7, 7) << 11)
                | field(word, 14, 14), 18)


def gp_load_half_offset(word):
    return sext(gp_common(word) | (field(word, 30, 21) << 1) | (field(word, 20, 20) << 11), 18)


def gp_store_half_offset(word):
    return sext(gp_common(word) | (field(word, 30, 25) << 5) | (field(word, 11, 8) << 1) | (field(word, 7, 7) << 11), 18)


def gp_load_word_offset(word):
    return sext((field(word, 31, 31) << 18) | (field(word, 30, 22) << 2) | (field(word, 21, 21) << 17)
                | (field(word, 20, 20) << 11) | (field(word, 19, 17) << 12) | (field(word, 16, 15) << 15), 19)


def gp_store_word_offset(word):
    return sext((field(word, 31, 31) << 18) | (field(word, 30, 25) << 5) | (field(word, 11, 9) << 2)
                | (field(word, 8, 8) << 17) | (field(word, 7, 7) << 11) | (field(word, 19, 17) << 12)
                | (field(word, 16, 15) << 15), 19)


def branch_offset(word):
    return sext((field(word, 31, 31) << 10) | (field(word, 29, 25) << 5) | (field(word, 11, 8) << 1), 11)


def decode(word):
    opcode = word & 0x7F
    rd, rs1, rs2 = field(word, 11, 7), field(word, 19, 15), field(word, 24, 20)
    if opcode == CUSTOM_0:
        kind = field(word, 13, 12)
        if kind == 3:
            return ("sbgp", {"rs2": rs2, "offset": gp_store_byte_offset(word)})
        return (("lbgp", "addigp", "lbugp")[kind], {"rd": rd, "offset": gp_load_byte_offset(word)})
    if opcode == CUSTOM_1:
        funct3 = field(word, 14, 12)
        if funct3 in (1, 5):
            return ("lhgp" if funct3 == 1 else "lhugp", {"rd": rd, "offset": gp_load_half_offset(word)})
        if funct3 == 0:
            return ("shgp", {"rs2": rs2, "offset": gp_store_half_offset(word)})
        if funct3 == 2:
            return ("lwgp", {"rd": rd, "offset": gp_load_word_offset(word)})
        if funct3 == 4:
            return ("swgp", {"rs2": rs2, "offset": gp_store_word_offset(word)})
        return None
    if opcode == CUSTOM_2:
        funct3 = field(word, 14, 12)
        if funct3 == 7:
            if field(word, 7, 7):
                return None
            return ("bbs" if field(word, 30, 30) else "bbc",
                    {"rs1": rs1, "bit": (field(word, 7, 7) << 5) | rs2, "offset": branch_offset(word)})
        if funct3 in (5, 6):
            constant = (field(word, 30, 30) << 6) | (field(word, 7, 7) << 5) | rs2
            return ("beqc" if funct3 == 5 else "bnec", {"rs1": rs1, "constant": constant, "offset": branch_offset(word)})
        if funct3 in (2, 3):
            if field(word, 31, 26) >= 32 or field(word, 25, 20) >= 32:
                return None
            return ("bfoz" if funct3 == 2 else "bfos",
                    {"rd": rd, "rs1": rs1, "msb": field(word, 31, 26), "lsb": field(word, 25, 20)})
        if funct3 == 0:
            name = {5: "lea.h", 6: "lea.w", 7: "lea.d"}.get(field(word, 31, 25))
            if name:
                return (name, {"rd": rd, "rs1": rs1, "rs2": rs2})
        return None
    if opcode == 0x7F and field(word, 14, 12) == 1:
        name = {0x60: "add64", 0x61: "sub64", 0x62: "maddr32", 0x78: "mulr64", 0x70: "mulsr64", 0x07: "pkbb16", 0x0F: "pkbt16", 0x17: "pktt16", 0x1F: "pktb16"}.get(field(word, 31, 25))
        if name:
            return (name, {"rd": rd, "rs1": rs1, "rs2": rs2})
    if opcode == 0x7F and field(word, 14, 12) == 0:
        name = {0x78: "minw", 0x79: "maxw"}.get(field(word, 31, 25))
        if name:
            return (name, {"rd": rd, "rs1": rs1, "rs2": rs2})
    return None


def bitfield(value, msb, lsb, signed):
    value &= MASK32
    if msb == 0:
        out = (value & 1) << lsb
        top = lsb
    elif msb < lsb:
        width = lsb - msb + 1
        out = (value & ((1 << width) - 1)) << msb
        top = lsb
    else:
        width = msb - lsb + 1
        out = (value >> lsb) & ((1 << width) - 1)
        top = width - 1
    if signed and (out >> top) & 1:
        out |= (MASK32 << (top + 1)) & MASK32
    return out & MASK32


class Machine:
    def read_reg(self, index):
        raise NotImplementedError

    def write_reg(self, index, value):
        raise NotImplementedError

    def load(self, address, size, signed):
        raise NotImplementedError

    def store(self, address, size, value):
        raise NotImplementedError


def compile_op(decoded):
    if decoded is None:
        return None
    name, op = decoded
    if name in ("bbc", "bbs"):
        rs1, bit, offset, want = op["rs1"], op["bit"], op["offset"], 1 if name == "bbs" else 0

        def bit_branch(machine, pc):
            taken = ((machine.read_reg(rs1) >> bit) & 1) == want
            return name, (pc + (offset if taken else 4)) & MASK32
        return bit_branch
    if name in ("beqc", "bnec"):
        rs1, constant, offset, equal = op["rs1"], op["constant"], op["offset"], name == "beqc"

        def constant_branch(machine, pc):
            taken = (machine.read_reg(rs1) == constant) == equal
            return name, (pc + (offset if taken else 4)) & MASK32
        return constant_branch
    if name in ("lbgp", "lbugp", "lhgp", "lhugp", "lwgp"):
        rd, offset = op["rd"], op["offset"]
        size = {"lbgp": 1, "lbugp": 1, "lhgp": 2, "lhugp": 2, "lwgp": 4}[name]
        signed = name in ("lbgp", "lhgp")

        def gp_load(machine, pc):
            value = machine.load((machine.read_reg(3) + offset) & MASK32, size, signed)
            if rd:
                machine.write_reg(rd, value & MASK32)
            return name, (pc + 4) & MASK32
        return gp_load
    if name in ("sbgp", "shgp", "swgp"):
        rs2, offset = op["rs2"], op["offset"]
        size = {"sbgp": 1, "shgp": 2, "swgp": 4}[name]

        def gp_store(machine, pc):
            machine.store((machine.read_reg(3) + offset) & MASK32, size, machine.read_reg(rs2) & MASK32)
            return name, (pc + 4) & MASK32
        return gp_store
    return lambda machine, pc: run(machine, pc, decoded)


def execute(machine, pc, word):
    decoded = decode(word)
    if decoded is None:
        return None
    return run(machine, pc, decoded)


def run(machine, pc, decoded):
    name, op = decoded
    gp = machine.read_reg(3)
    next_pc = (pc + 4) & MASK32

    def reg(i):
        return machine.read_reg(i) & MASK32

    def put(i, value):
        if i:
            machine.write_reg(i, value & MASK32)

    if name == "addigp":
        put(op["rd"], gp + op["offset"])
    elif name in ("lbgp", "lbugp", "lhgp", "lhugp", "lwgp"):
        size = {"lbgp": 1, "lbugp": 1, "lhgp": 2, "lhugp": 2, "lwgp": 4}[name]
        put(op["rd"], machine.load((gp + op["offset"]) & MASK32, size, name in ("lbgp", "lhgp")))
    elif name in ("sbgp", "shgp", "swgp"):
        size = {"sbgp": 1, "shgp": 2, "swgp": 4}[name]
        machine.store((gp + op["offset"]) & MASK32, size, reg(op["rs2"]))
    elif name in ("bbc", "bbs"):
        taken = ((reg(op["rs1"]) >> op["bit"]) & 1) == (1 if name == "bbs" else 0)
        if taken:
            next_pc = (pc + op["offset"]) & MASK32
    elif name in ("beqc", "bnec"):
        taken = (reg(op["rs1"]) == op["constant"]) == (name == "beqc")
        if taken:
            next_pc = (pc + op["offset"]) & MASK32
    elif name in ("bfoz", "bfos"):
        put(op["rd"], bitfield(reg(op["rs1"]), op["msb"], op["lsb"], name == "bfos"))
    elif name.startswith("lea."):
        shift = {"lea.h": 1, "lea.w": 2, "lea.d": 3}[name]
        put(op["rd"], reg(op["rs1"]) + (reg(op["rs2"]) << shift))
    elif name in ("minw", "maxw"):
        a, b = sext(reg(op["rs1"]), 32), sext(reg(op["rs2"]), 32)
        put(op["rd"], min(a, b) if name == "minw" else max(a, b))
    elif name.startswith("pk"):
        a, b = reg(op["rs1"]), reg(op["rs2"])
        a = a >> 16 if name[2] == "t" else a & 0xFFFF
        b = b >> 16 if name[3] == "t" else b & 0xFFFF
        put(op["rd"], (a << 16) | b)
    elif name == "maddr32":
        put(op["rd"], reg(op["rd"]) + reg(op["rs1"]) * reg(op["rs2"]))
    elif name in ("add64", "sub64", "mulr64", "mulsr64"):
        if name in ("mulr64", "mulsr64"):
            a, b = reg(op["rs1"]), reg(op["rs2"])
            if name == "mulsr64":
                a, b = sext(a, 32), sext(b, 32)
            value = a * b
        else:
            def pair(i):
                i &= ~1
                return reg(i) | (reg(i + 1) << 32)
            a, b = pair(op["rs1"]), pair(op["rs2"])
            value = a + b if name == "add64" else a - b
        rd = op["rd"] & ~1
        put(rd, value)
        put(rd + 1, value >> 32)
    return name, next_pc
