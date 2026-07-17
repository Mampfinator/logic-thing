# Instructions

`writable` refers to any writable value; be that a register or a memory address.

`readable` refers to any readable value; so all writable values + constants.

Syntax overview:

- Constant: `42`
- Register: `r0`
- Constant memory address: `[0]`
- Memory address stored in a register: `[r0]`

| instruction (+ syntax)      | opcode | notes |
| --------------------------- | ------ | ----- |
| `mov <readable> <writable>` | #01    |       |
| `jmp <readable>`            | #02    |       |
